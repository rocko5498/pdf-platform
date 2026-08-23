//! Plugin lifecycle manager. [SDS §11, ADR-014, M11]
//!
//! Manages plugin discovery, loading, invocation, and unloading.
//! Enforces capability grants, CPU quotas, and a circuit breaker
//! for repeated failures.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::grant::GrantStore;
use crate::manifest::{Capability, PluginManifest, parse_manifest};
use crate::runtime::{PluginRuntime, RuntimeError};
use crate::PluginState;

/// Maximum consecutive failures before the circuit breaker trips.
const CIRCUIT_BREAKER_THRESHOLD: u32 = 3;

/// Cooldown period after the circuit breaker trips.
const CIRCUIT_BREAKER_COOLDOWN: Duration = Duration::from_secs(60);

/// UI contributions from a loaded plugin.
#[derive(Debug, Default, Clone)]
pub struct PluginUI {
    /// Panel contributions (serialized JSON).
    pub panels_json: String,
    /// Tool contributions (serialized JSON).
    pub tools_json: String,
}

/// A loaded plugin instance's metadata.
pub(crate) struct LoadedPlugin {
    /// The plugin's manifest.
    pub(crate) manifest: PluginManifest,
    /// Current state.
    pub(crate) state: PluginState,
    /// Capability grants.
    pub(crate) grants: GrantStore,
    /// UI contributions.
    pub(crate) ui: PluginUI,
    /// Consecutive failure count (for circuit breaker).
    pub(crate) failure_count: u32,
    /// When the circuit breaker last tripped.
    pub(crate) circuit_breaker_tripped_at: Option<Instant>,
    /// The plugin's own WebAssembly, from the package that carried the manifest.
    pub(crate) wasm: Vec<u8>,
    /// Whether the guest's shutdown hook ran during graceful unload. [SDS §11.5]
    pub(crate) shutdown_hook_called: bool,
    pub(crate) module: Option<wasmtime::Module>,
    pub(crate) store: Option<wasmtime::Store<crate::runtime::PluginInstanceState>>,
    pub(crate) instance: Option<wasmtime::Instance>,
}

impl LoadedPlugin {
    /// Count a fault against the circuit breaker. [SDS §11.5]
    fn note_failure(&mut self) {
        self.failure_count += 1;
        if self.failure_count >= CIRCUIT_BREAKER_THRESHOLD {
            self.circuit_breaker_tripped_at = Some(Instant::now());
        }
    }
}

/// The plugin manager. [SDS §2.2.7]
///
/// Owns the WASM runtime and all loaded plugin instances.
/// Routes plugin requests through the capability broker.
pub struct PluginManager {
    /// The WASM runtime.
    runtime: PluginRuntime,
    /// Loaded plugin instances keyed by plugin ID.
    plugins: HashMap<String, LoadedPlugin>,
}

impl PluginManager {
    /// Create a new plugin manager.
    pub fn new() -> Result<Self, RuntimeError> {
        let runtime = PluginRuntime::new()?;
        Ok(Self {
            runtime,
            plugins: HashMap::new(),
        })
    }

    /// Create a manager with custom runtime configuration.
    pub fn with_config(_fuel_epoch: u64) -> Result<Self, RuntimeError> {
        let runtime = PluginRuntime::new()?;
        Ok(Self {
            runtime,
            plugins: HashMap::new(),
        })
    }

    /// Discover and validate a plugin from JSON manifest bytes.
    ///
    /// Returns the parsed manifest on success.
    pub fn discover(&self, manifest_bytes: &[u8]) -> Result<PluginManifest, PluginError> {
        parse_manifest(manifest_bytes).map_err(PluginError::InvalidManifest)
    }

    /// Enable a plugin with the given capability grants and its module bytes.
    ///
    /// `module_wasm` is the plugin's own WebAssembly, taken from the package
    /// that carried the manifest (`SDS §11.1`). The manager does not read it
    /// from disk: no canonical document specifies a plugin directory layout, so
    /// the caller that opened the package supplies the bytes rather than this
    /// crate inventing a location for them (`AI-1`).
    ///
    /// The plugin is not instantiated yet — that happens on first use
    /// (`SDS §11.2`).
    pub fn enable(
        &mut self,
        manifest: PluginManifest,
        grants: GrantStore,
        module_wasm: Vec<u8>,
    ) -> Result<(), PluginError> {
        let id = manifest.id.clone();

        if module_wasm.is_empty() {
            return Err(PluginError::EmptyModule(id));
        }

        // Validate that granted capabilities are declared in the manifest.
        for grant in grants.grants_for(&id) {
            if grant.granted && !manifest.capabilities.contains(&grant.capability) {
                return Err(PluginError::UndeclaredCapability {
                    plugin_id: id,
                    capability: format!("{:?}", grant.capability),
                });
            }
        }

        let plugin = LoadedPlugin {
            manifest,
            state: PluginState::Enabled,
            grants,
            ui: PluginUI::default(),
            failure_count: 0,
            circuit_breaker_tripped_at: None,
            wasm: module_wasm,
            shutdown_hook_called: false,
            module: None,
            store: None,
            instance: None,
        };

        self.plugins.insert(id, plugin);
        Ok(())
    }

    /// Load a plugin: compile its module, instantiate it, and call its entry
    /// point. [SDS §11.2, FR-PLUG-1, ADR-014]
    ///
    /// This used to compile a one-instruction stub and instantiate *that*, so
    /// enabling a plugin exercised the wasmtime pipeline and never ran a line
    /// of the plugin's code. The module now comes from the plugin, and the
    /// entry point the WIT world names — `run`, no parameters, no result — is
    /// called before this returns, so a plugin that fails to start fails here
    /// rather than appearing to run.
    ///
    /// The module is core WebAssembly linked against the host functions in
    /// `host_calls`. Instantiating a *component* against the WIT world is not
    /// implemented; a component fails here with a wasmtime error rather than
    /// silently doing nothing.
    pub fn load(&mut self, plugin_id: &str) -> Result<(), PluginError> {
        let plugin = self
            .plugins
            .get_mut(plugin_id)
            .ok_or_else(|| PluginError::NotFound(plugin_id.into()))?;

        // Check circuit breaker.
        if plugin.failure_count >= CIRCUIT_BREAKER_THRESHOLD {
            if let Some(tripped_at) = plugin.circuit_breaker_tripped_at {
                if tripped_at.elapsed() < CIRCUIT_BREAKER_COOLDOWN {
                    return Err(PluginError::CircuitBreakerOpen {
                        plugin_id: plugin_id.into(),
                        cooldown_remaining: CIRCUIT_BREAKER_COOLDOWN
                            .checked_sub(tripped_at.elapsed())
                            .unwrap_or_default(),
                    });
                }
                // Cooldown expired — reset.
                plugin.failure_count = 0;
                plugin.circuit_breaker_tripped_at = None;
            }
        }

        let module = match wasmtime::Module::new(self.runtime.engine(), &plugin.wasm) {
            Ok(module) => module,
            Err(error) => {
                plugin.note_failure();
                return Err(PluginError::Runtime(RuntimeError::Compile(
                    error.to_string(),
                )));
            }
        };

        let mut store = self.runtime.create_store(
            plugin.manifest.clone(),
            plugin.grants.clone(),
            crate::runtime::InstanceConfig::default(),
        );

        // Only granted host functions exist in this instance. [ADR-014 §2]
        let granted: Vec<Capability> = plugin
            .grants
            .grants_for(plugin_id)
            .into_iter()
            .filter(|grant| grant.granted)
            .map(|grant| grant.capability.clone())
            .collect();

        let mut linker = wasmtime::Linker::new(self.runtime.engine());
        crate::host_calls::link_host_functions(&mut linker, &granted)
            .map_err(|e| PluginError::Runtime(RuntimeError::HostCall(e.to_string())))?;

        let instance = match linker.instantiate(&mut store, &module) {
            Ok(instance) => instance,
            Err(error) => {
                plugin.note_failure();
                return Err(PluginError::Runtime(RuntimeError::Instantiate(
                    error.to_string(),
                )));
            }
        };

        // The host calls the plugin's entry point. [SDS §11.2 step 3]
        let run = match instance.get_typed_func::<(), ()>(&mut store, "run") {
            Ok(run) => run,
            Err(_) => {
                plugin.note_failure();
                return Err(PluginError::MissingExport {
                    plugin_id: plugin_id.into(),
                    export: "run".into(),
                });
            }
        };
        if let Err(error) = run.call(&mut store, ()) {
            plugin.note_failure();
            return Err(PluginError::GuestTrap {
                plugin_id: plugin_id.into(),
                detail: error.to_string(),
            });
        }

        store.data_mut().initialized = true;
        plugin.module = Some(module);
        plugin.instance = Some(instance);
        plugin.store = Some(store);
        plugin.state = PluginState::Running;
        Ok(())
    }

    /// Invoke a plugin-registered tool action by calling the guest export of
    /// that name. [SDS §11.4, FR-PLUG-3]
    ///
    /// `args_json` is **not** forwarded: the WIT world defines `run: func()`
    /// and no argument-passing ABI for actions, so there is nowhere to put it.
    /// The response says `"args_passed":false` rather than echoing the
    /// arguments back and implying the plugin saw them (`GR-8`). This method
    /// used to return exactly that echo without calling the guest at all.
    pub fn invoke_action(
        &mut self,
        plugin_id: &str,
        action_id: &str,
        _args_json: &str,
    ) -> Result<String, PluginError> {
        let plugin = self
            .plugins
            .get_mut(plugin_id)
            .ok_or_else(|| PluginError::NotFound(plugin_id.into()))?;

        if plugin.state != PluginState::Running {
            return Err(PluginError::NotRunning(plugin_id.into()));
        }

        // Check circuit breaker.
        if plugin.failure_count >= CIRCUIT_BREAKER_THRESHOLD {
            return Err(PluginError::CircuitBreakerOpen {
                plugin_id: plugin_id.into(),
                cooldown_remaining: Duration::ZERO,
            });
        }

        let Some(instance) = plugin.instance else {
            return Err(PluginError::NotRunning(plugin_id.into()));
        };

        let outcome = {
            let store = plugin
                .store
                .as_mut()
                .ok_or_else(|| PluginError::NotRunning(plugin_id.into()))?;

            if self.runtime.consume_fuel(store, 1000).is_none() {
                Err(PluginError::Runtime(RuntimeError::Preempted))
            } else {
                match instance.get_typed_func::<(), ()>(&mut *store, action_id) {
                    Err(_) => Err(PluginError::MissingExport {
                        plugin_id: plugin_id.into(),
                        export: action_id.into(),
                    }),
                    Ok(action) => action.call(&mut *store, ()).map_err(|error| {
                        PluginError::GuestTrap {
                            plugin_id: plugin_id.into(),
                            detail: error.to_string(),
                        }
                    }),
                }
            }
        };

        match outcome {
            Ok(()) => {
                plugin.failure_count = 0;
                plugin.circuit_breaker_tripped_at = None;
                Ok(format!(
                    r#"{{"status":"ok","plugin":"{plugin_id}","action":"{action_id}","args_passed":false}}"#
                ))
            }
            Err(error) => {
                plugin.note_failure();
                Err(error)
            }
        }
    }

    /// Unload a plugin, transitioning it to Disabled.
    pub fn unload(&mut self, plugin_id: &str) -> Result<(), PluginError> {
        let plugin = self
            .plugins
            .get_mut(plugin_id)
            .ok_or_else(|| PluginError::NotFound(plugin_id.into()))?;

        // Graceful termination calls the guest's shutdown hook first, if it has
        // one; a plugin that traps on the way out is still unloaded. [SDS §11.5]
        let mut called = false;
        if let (Some(instance), Some(store)) = (plugin.instance, plugin.store.as_mut()) {
            if let Ok(shutdown) = instance.get_typed_func::<(), ()>(&mut *store, "shutdown") {
                called = shutdown.call(&mut *store, ()).is_ok();
            }
            store.data_mut().shut_down = true;
        }
        plugin.shutdown_hook_called = called;

        plugin.instance = None;
        plugin.store = None;
        plugin.state = PluginState::Disabled;
        plugin.ui = PluginUI::default();
        Ok(())
    }

    /// Get the current state of a plugin.
    pub fn state(&self, plugin_id: &str) -> Option<&PluginState> {
        self.plugins.get(plugin_id).map(|p| &p.state)
    }

    /// Get UI contributions from a plugin.
    pub fn ui(&self, plugin_id: &str) -> Option<&PluginUI> {
        self.plugins.get(plugin_id).map(|p| &p.ui)
    }

    /// Get all loaded plugin IDs.
    pub fn plugin_ids(&self) -> Vec<&str> {
        self.plugins.keys().map(|s| s.as_str()).collect()
    }

    /// Check if a plugin has a specific capability granted.
    pub fn has_capability(&self, plugin_id: &str, capability: &Capability) -> bool {
        self.plugins
            .get(plugin_id)
            .map(|p| p.grants.is_granted(plugin_id, capability))
            .unwrap_or(false)
    }

    /// Record a failure for the circuit breaker.
    pub fn record_failure(&mut self, plugin_id: &str) {
        if let Some(plugin) = self.plugins.get_mut(plugin_id) {
            plugin.failure_count += 1;
            if plugin.failure_count >= CIRCUIT_BREAKER_THRESHOLD {
                plugin.circuit_breaker_tripped_at = Some(Instant::now());
                // Note: state stays Running; the circuit breaker is checked
                // separately in invoke_action. The state only changes when
                // we actually fail an invocation.
            }
        }
    }

    /// Record a success (resets the failure count).
    pub fn record_success(&mut self, plugin_id: &str) {
        if let Some(plugin) = self.plugins.get_mut(plugin_id) {
            plugin.failure_count = 0;
            plugin.circuit_breaker_tripped_at = None;
        }
    }

    /// Remove a plugin entirely (e.g., on uninstall).
    ///
    /// Returns `true` if the plugin was found and removed.
    pub fn remove(&mut self, plugin_id: &str) -> bool {
        self.plugins.remove(plugin_id).is_some()
    }

    /// Get a reference to the runtime.
    pub fn runtime(&self) -> &PluginRuntime {
        &self.runtime
    }
}

/// Errors from plugin management.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginError {
    /// Plugin not found.
    NotFound(String),
    /// Invalid manifest.
    InvalidManifest(crate::manifest::ManifestError),
    /// Plugin is not in Running state.
    NotRunning(String),
    /// Capability not declared in manifest but grant attempted.
    UndeclaredCapability {
        /// Plugin ID.
        plugin_id: String,
        /// Capability name.
        capability: String,
    },
    /// Circuit breaker is open (too many failures).
    CircuitBreakerOpen {
        /// Plugin ID.
        plugin_id: String,
        /// Time remaining before cooldown expires.
        cooldown_remaining: Duration,
    },
    /// The plugin was enabled with no module bytes.
    EmptyModule(String),
    /// The plugin's module does not export something the host must call.
    MissingExport {
        /// Plugin ID.
        plugin_id: String,
        /// The export that was looked up.
        export: String,
    },
    /// Guest code trapped.
    GuestTrap {
        /// Plugin ID.
        plugin_id: String,
        /// The trap, as wasmtime reported it.
        detail: String,
    },
    /// Runtime error.
    Runtime(RuntimeError),
}

impl std::fmt::Display for PluginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(id) => write!(f, "plugin not found: {id}"),
            Self::InvalidManifest(e) => write!(f, "invalid manifest: {e}"),
            Self::NotRunning(id) => write!(f, "plugin not running: {id}"),
            Self::UndeclaredCapability {
                plugin_id,
                capability,
            } => write!(
                f,
                "capability '{capability}' not declared in manifest for {plugin_id}"
            ),
            Self::CircuitBreakerOpen {
                plugin_id,
                cooldown_remaining,
            } => write!(
                f,
                "circuit breaker open for {plugin_id}, cooldown {cooldown_remaining:.0?}"
            ),
            Self::EmptyModule(id) => write!(
                f,
                "plugin {id} was enabled with no module bytes; the package must supply the plugin's WebAssembly"
            ),
            Self::MissingExport { plugin_id, export } => {
                write!(f, "plugin {plugin_id} does not export '{export}'")
            }
            Self::GuestTrap { plugin_id, detail } => {
                write!(f, "plugin {plugin_id} trapped: {detail}")
            }
            Self::Runtime(e) => write!(f, "runtime error: {e}"),
        }
    }
}

impl std::error::Error for PluginError {}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{Capability, PanelContribution, ToolContribution, JobTypeContribution};

    fn test_manifest() -> PluginManifest {
        PluginManifest {
            id: "com.example.test".into(),
            name: "Test Plugin".into(),
            version: "1.0.0".into(),
            author: "Author".into(),
            description: "A test plugin".into(),
            wit_world: "pdf-platform:plugin@1".into(),
            capabilities: vec![Capability::ReadText, Capability::Annotate],
            panels: vec![PanelContribution {
                id: "info".into(),
                label: "Info".into(),
                position: Default::default(),
            }],
            tools: vec![ToolContribution {
                id: "count".into(),
                label: "Count Words".into(),
                menu_path: "Plugins > Count".into(),
            }],
            job_types: vec![JobTypeContribution {
                id: "analyze".into(),
                label: "Analyze".into(),
            }],
        }
    }

    /// A guest that leaves evidence in its own memory of having run.
    ///
    /// `run` writes a marker at offset 0 and whatever `host_get_page_count`
    /// returned at offset 4; `count` writes 7 at offset 8. A host that
    /// instantiates a stub, or never calls the guest, leaves all three zero —
    /// which is what every one of these assertions is for. [T-10]
    const RECORDING_GUEST: &str = r#"
        (module
          (import "env" "host_get_page_count" (func $page_count (result i32)))
          (memory (export "memory") 1)
          (func (export "run")
            (i32.store (i32.const 0) (i32.const 0x00c0ffee))
            (i32.store (i32.const 4) (call $page_count)))
          (func (export "count")
            (i32.store (i32.const 8) (i32.const 7)))
          (func (export "shutdown")
            (i32.store (i32.const 12) (i32.const 9))))
    "#;

    /// A guest whose entry point traps.
    const TRAPPING_GUEST: &str = r#"(module (func (export "run") (unreachable)))"#;

    /// A well-formed module that is not a plugin: no `run`.
    const GUEST_WITHOUT_RUN: &str = r#"(module (func (export "other") (nop)))"#;

    fn guest(wat: &str) -> Vec<u8> {
        wat.as_bytes().to_vec()
    }

    /// Grants that let `RECORDING_GUEST` link its one import.
    fn read_text_granted() -> GrantStore {
        let mut grants = GrantStore::new();
        grants.grant("com.example.test", Capability::ReadText);
        grants
    }

    /// Read four bytes of the loaded guest's linear memory.
    fn guest_memory_u32(manager: &mut PluginManager, plugin_id: &str, offset: usize) -> u32 {
        let plugin = manager.plugins.get_mut(plugin_id).expect("plugin present");
        let instance = plugin.instance.expect("instance present");
        let store = plugin.store.as_mut().expect("store present");
        let memory = instance
            .get_memory(&mut *store, "memory")
            .expect("guest exports memory");
        let mut bytes = [0u8; 4];
        memory.read(&*store, offset, &mut bytes).expect("read guest memory");
        u32::from_le_bytes(bytes)
    }

    fn manifest_json() -> String {
        r#"{
            "id": "com.example.test",
            "name": "Test Plugin",
            "version": "1.0.0",
            "author": "Author",
            "description": "A test plugin",
            "wit_world": "pdf-platform:plugin@1",
            "capabilities": [{"type": "ReadText"}, {"type": "Annotate"}],
            "panels": [{"id": "info", "label": "Info"}],
            "tools": [{"id": "count", "label": "Count Words", "menu_path": "Plugins > Count"}],
            "job_types": [{"id": "analyze", "label": "Analyze"}]
        }"#
        .into()
    }

    #[test]
    fn discover_valid_manifest() {
        let manager = PluginManager::new().unwrap();
        let manifest = manager.discover(manifest_json().as_bytes()).unwrap();
        assert_eq!(manifest.id, "com.example.test");
        assert_eq!(manifest.capabilities.len(), 2);
    }

    #[test]
    fn discover_invalid_manifest() {
        let manager = PluginManager::new().unwrap();
        let result = manager.discover(b"not json");
        assert!(result.is_err());
    }

    #[test]
    fn enable_plugin() {
        let mut manager = PluginManager::new().unwrap();
        let manifest = test_manifest();
        let mut grants = GrantStore::new();
        grants.grant("com.example.test", Capability::ReadText);
        grants.grant("com.example.test", Capability::Annotate);

        manager.enable(manifest, grants, guest(RECORDING_GUEST)).unwrap();
        assert_eq!(
            manager.state("com.example.test"),
            Some(&PluginState::Enabled)
        );
    }

    #[test]
    fn enable_rejects_undeclared_capability() {
        let mut manager = PluginManager::new().unwrap();
        let manifest = test_manifest();
        let mut grants = GrantStore::new();
        grants.grant("com.example.test", Capability::Network); // not declared

        let result = manager.enable(manifest, grants, guest(RECORDING_GUEST));
        assert!(result.is_err());
        assert!(matches!(
            result,
            Err(PluginError::UndeclaredCapability { .. })
        ));
    }

    #[test]
    fn load_runs_the_plugins_own_code() {
        // The old test asserted only that the state became Running, which it
        // did while the host compiled a `(nop)` stub and ignored the plugin.
        let mut manager = PluginManager::new().unwrap();
        let mut grants = GrantStore::new();
        grants.grant("com.example.test", Capability::ReadText);
        manager
            .enable(test_manifest(), grants, guest(RECORDING_GUEST))
            .unwrap();

        manager.load("com.example.test").unwrap();

        assert_eq!(
            manager.state("com.example.test"),
            Some(&PluginState::Running)
        );
        assert_eq!(
            guest_memory_u32(&mut manager, "com.example.test", 0),
            0x00c0_ffee,
            "the guest's `run` never executed: its marker is not in its memory"
        );
    }

    #[test]
    fn an_ungranted_host_function_is_absent_from_the_instance() {
        // ADR-014 §2 and SDS §11.3: undeclared capabilities are *unlinkable*,
        // not denied at call time. The linker used to wire every host function
        // into every plugin and check the grant inside the call, so an
        // ungranted plugin still held the import and got -1 back. [T-12]
        let mut manager = PluginManager::new().unwrap();
        manager
            .enable(test_manifest(), GrantStore::new(), guest(RECORDING_GUEST))
            .unwrap();

        let result = manager.load("com.example.test");

        let Err(PluginError::Runtime(RuntimeError::Instantiate(detail))) = &result else {
            panic!("expected an unknown-import failure, got {result:?}");
        };
        assert!(
            detail.contains("host_get_page_count"),
            "the error must name the import that was absent: {detail}"
        );
    }

    #[test]
    fn a_granted_host_function_is_callable_from_the_guest() {
        // The other half of the seam: with ReadText granted, the same guest
        // links and the host's return value lands in guest memory. [T-12]
        let mut manager = PluginManager::new().unwrap();
        manager
            .enable(test_manifest(), read_text_granted(), guest(RECORDING_GUEST))
            .unwrap();
        manager.load("com.example.test").unwrap();

        // The store reports 0 pages until the coordinator fills it in, so the
        // marker at offset 0 is what distinguishes "ran" from "never ran".
        assert_eq!(
            guest_memory_u32(&mut manager, "com.example.test", 0),
            0x00c0_ffee
        );
        assert_eq!(
            guest_memory_u32(&mut manager, "com.example.test", 4),
            0,
            "a granted host_get_page_count must not report denial"
        );
    }

    #[test]
    fn a_trapping_guest_fails_the_load_and_counts_a_failure() {
        let mut manager = PluginManager::new().unwrap();
        manager
            .enable(test_manifest(), GrantStore::new(), guest(TRAPPING_GUEST))
            .unwrap();

        let result = manager.load("com.example.test");

        assert!(
            matches!(result, Err(PluginError::GuestTrap { .. })),
            "expected a trap, got {result:?}"
        );
        assert_eq!(
            manager.state("com.example.test"),
            Some(&PluginState::Enabled),
            "a plugin that trapped on start must not be reported as Running"
        );
        assert_eq!(
            manager.plugins["com.example.test"].failure_count, 1,
            "the circuit breaker must see the fault"
        );
    }

    #[test]
    fn a_module_without_run_is_rejected() {
        let mut manager = PluginManager::new().unwrap();
        manager
            .enable(test_manifest(), GrantStore::new(), guest(GUEST_WITHOUT_RUN))
            .unwrap();

        let result = manager.load("com.example.test");

        assert!(
            matches!(&result, Err(PluginError::MissingExport { export, .. }) if export == "run"),
            "expected a missing-`run` error, got {result:?}"
        );
    }

    #[test]
    fn a_module_that_is_not_webassembly_is_rejected() {
        let mut manager = PluginManager::new().unwrap();
        manager
            .enable(test_manifest(), GrantStore::new(), b"not wasm at all".to_vec())
            .unwrap();

        assert!(matches!(
            manager.load("com.example.test"),
            Err(PluginError::Runtime(RuntimeError::Compile(_)))
        ));
    }

    #[test]
    fn enable_rejects_a_plugin_with_no_module() {
        let mut manager = PluginManager::new().unwrap();
        let result = manager.enable(test_manifest(), GrantStore::new(), Vec::new());
        assert!(matches!(result, Err(PluginError::EmptyModule(_))));
    }

    #[test]
    fn load_nonexistent_plugin() {
        let mut manager = PluginManager::new().unwrap();
        let result = manager.load("nonexistent");
        assert!(matches!(result, Err(PluginError::NotFound(_))));
    }

    #[test]
    fn invoke_action_calls_the_named_export() {
        let mut manager = PluginManager::new().unwrap();
        manager
            .enable(test_manifest(), read_text_granted(), guest(RECORDING_GUEST))
            .unwrap();
        manager.load("com.example.test").unwrap();

        manager
            .invoke_action("com.example.test", "count", "{}")
            .unwrap();

        assert_eq!(
            guest_memory_u32(&mut manager, "com.example.test", 8),
            7,
            "`count` did not run: the host returned success without calling it"
        );
    }

    #[test]
    fn invoking_an_action_the_plugin_does_not_export_fails() {
        // The old implementation returned `{"status":"ok",...}` for any action
        // name at all, including ones no plugin had ever declared. [GR-8]
        let mut manager = PluginManager::new().unwrap();
        manager
            .enable(test_manifest(), read_text_granted(), guest(RECORDING_GUEST))
            .unwrap();
        manager.load("com.example.test").unwrap();

        let result = manager.invoke_action("com.example.test", "no_such_action", "{}");

        assert!(
            matches!(&result, Err(PluginError::MissingExport { export, .. })
                if export == "no_such_action"),
            "expected a missing-export error, got {result:?}"
        );
    }

    #[test]
    fn unload_calls_the_guest_shutdown_hook() {
        let mut manager = PluginManager::new().unwrap();
        manager
            .enable(test_manifest(), read_text_granted(), guest(RECORDING_GUEST))
            .unwrap();
        manager.load("com.example.test").unwrap();
        assert!(!manager.plugins["com.example.test"].shutdown_hook_called);

        manager.unload("com.example.test").unwrap();

        assert!(
            manager.plugins["com.example.test"].shutdown_hook_called,
            "the guest's shutdown hook was never called"
        );
        assert_eq!(
            manager.state("com.example.test"),
            Some(&PluginState::Disabled)
        );
    }

    #[test]
    fn invoke_action_not_running() {
        let mut manager = PluginManager::new().unwrap();
        let manifest = test_manifest();
        manager
            .enable(manifest, read_text_granted(), guest(RECORDING_GUEST))
            .unwrap();
        // Don't load — still Enabled.

        let result = manager.invoke_action("com.example.test", "count", "{}");
        assert!(matches!(result, Err(PluginError::NotRunning(_))));
    }

    #[test]
    fn unload_plugin() {
        let mut manager = PluginManager::new().unwrap();
        let manifest = test_manifest();
        manager
            .enable(manifest, read_text_granted(), guest(RECORDING_GUEST))
            .unwrap();
        manager.load("com.example.test").unwrap();

        manager.unload("com.example.test").unwrap();
        assert_eq!(
            manager.state("com.example.test"),
            Some(&PluginState::Disabled)
        );
    }

    #[test]
    fn circuit_breaker_trips() {
        let mut manager = PluginManager::new().unwrap();
        let manifest = test_manifest();
        manager
            .enable(manifest, read_text_granted(), guest(RECORDING_GUEST))
            .unwrap();
        manager.load("com.example.test").unwrap();

        // Record 3 failures to trip the breaker.
        for _ in 0..CIRCUIT_BREAKER_THRESHOLD {
            manager.record_failure("com.example.test");
        }

        let result = manager.invoke_action("com.example.test", "count", "{}");
        assert!(matches!(result, Err(PluginError::CircuitBreakerOpen { .. })));
    }

    #[test]
    fn circuit_breaker_resets_on_success() {
        let mut manager = PluginManager::new().unwrap();
        let manifest = test_manifest();
        manager
            .enable(manifest, read_text_granted(), guest(RECORDING_GUEST))
            .unwrap();
        manager.load("com.example.test").unwrap();

        // Record 2 failures (below threshold).
        manager.record_failure("com.example.test");
        manager.record_failure("com.example.test");

        // Success resets the count.
        manager.record_success("com.example.test");

        // Should still be able to invoke.
        let result = manager.invoke_action("com.example.test", "count", "{}");
        assert!(result.is_ok());
    }

    #[test]
    fn plugin_ids() {
        let mut manager = PluginManager::new().unwrap();
        let manifest = test_manifest();
        manager
            .enable(manifest, read_text_granted(), guest(RECORDING_GUEST))
            .unwrap();

        let ids = manager.plugin_ids();
        assert_eq!(ids, vec!["com.example.test"]);
    }

    #[test]
    fn has_capability() {
        let mut manager = PluginManager::new().unwrap();
        let manifest = test_manifest();
        let mut grants = GrantStore::new();
        grants.grant("com.example.test", Capability::ReadText);
        manager.enable(manifest, grants, guest(RECORDING_GUEST)).unwrap();

        assert!(manager.has_capability("com.example.test", &Capability::ReadText));
        assert!(!manager.has_capability("com.example.test", &Capability::Annotate));
    }

    #[test]
    fn remove_plugin() {
        let mut manager = PluginManager::new().unwrap();
        let manifest = test_manifest();
        manager
            .enable(manifest, read_text_granted(), guest(RECORDING_GUEST))
            .unwrap();

        let removed = manager.remove("com.example.test");
        assert!(removed);
        assert!(manager.state("com.example.test").is_none());
    }
}
