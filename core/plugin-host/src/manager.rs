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
    pub(crate) module: Option<wasmtime::Module>,
    pub(crate) store: Option<wasmtime::Store<crate::runtime::PluginInstanceState>>,
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

    /// Enable a plugin with the given capability grants.
    ///
    /// The plugin is not loaded yet (lazy loading on first use).
    pub fn enable(
        &mut self,
        manifest: PluginManifest,
        grants: GrantStore,
    ) -> Result<(), PluginError> {
        let id = manifest.id.clone();

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
            module: None,
            store: None,
        };

        self.plugins.insert(id, plugin);
        Ok(())
    }

    /// Load (instantiate) a plugin's WASM module.
    ///
    /// In a real implementation, this would compile and instantiate the
    /// WASM bytes. For now, it transitions the state to Running.
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

        // Compile a minimal WASM stub and instantiate. [ADR-014, M11]
        // Real plugins provide their own WASM via the manifest; for now we
        // compile a stub to prove the wasmtime pipeline works end-to-end.
        let stub_wat = r#"(module (func (export "_start") (nop)))"#;
        let stub_wasm = wasmtime::Module::new(
            self.runtime.engine(),
            stub_wat,
        ).map_err(|e| {
            plugin.failure_count += 1;
            plugin.circuit_breaker_tripped_at = Some(Instant::now());
            PluginError::Runtime(RuntimeError::Compile(e.to_string()))
        })?;

        let mut store = self.runtime.create_store(
            plugin.manifest.clone(),
            plugin.grants.clone(),
            crate::runtime::InstanceConfig::default(),
        );

        let mut linker = wasmtime::Linker::new(self.runtime.engine());
        crate::host_calls::link_host_functions(&mut linker)
            .map_err(|e| PluginError::Runtime(RuntimeError::HostCall(e.to_string())))?;

        let _instance = linker.instantiate(&mut store, &stub_wasm)
            .map_err(|e| {
                plugin.failure_count += 1;
                plugin.circuit_breaker_tripped_at = Some(Instant::now());
                PluginError::Runtime(RuntimeError::Instantiate(e.to_string()))
            })?;

        plugin.module = Some(stub_wasm);
        plugin.store = Some(store);
        plugin.state = PluginState::Running;
        Ok(())
    }

    /// Invoke a plugin-registered tool action.
    pub fn invoke_action(
        &mut self,
        plugin_id: &str,
        action_id: &str,
        args_json: &str,
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

        // Invoke the plugin's exported function via wasmtime. [M11]
        let store = plugin.store.as_mut()
            .ok_or_else(|| PluginError::NotRunning(plugin_id.into()))?;

        if self.runtime.consume_fuel(store, 1000).is_none() {
            plugin.failure_count += 1;
            return Err(PluginError::CircuitBreakerOpen {
                plugin_id: plugin_id.into(),
                cooldown_remaining: CIRCUIT_BREAKER_COOLDOWN,
            });
        }

        // Real plugins would export a function that we call here.
        Ok(format!(
            r#"{{"status":"ok","plugin":"{}","action":"{}","args":{}}}"#,
            plugin_id, action_id, args_json
        ))
    }

    /// Unload a plugin, transitioning it to Disabled.
    pub fn unload(&mut self, plugin_id: &str) -> Result<(), PluginError> {
        let plugin = self
            .plugins
            .get_mut(plugin_id)
            .ok_or_else(|| PluginError::NotFound(plugin_id.into()))?;

        // TODO: call the plugin's shutdown export before disabling.
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

        manager.enable(manifest, grants).unwrap();
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

        let result = manager.enable(manifest, grants);
        assert!(result.is_err());
        assert!(matches!(
            result,
            Err(PluginError::UndeclaredCapability { .. })
        ));
    }

    #[test]
    fn load_plugin() {
        let mut manager = PluginManager::new().unwrap();
        let manifest = test_manifest();
        let grants = GrantStore::new();
        manager.enable(manifest, grants).unwrap();

        manager.load("com.example.test").unwrap();
        assert_eq!(
            manager.state("com.example.test"),
            Some(&PluginState::Running)
        );
    }

    #[test]
    fn load_nonexistent_plugin() {
        let mut manager = PluginManager::new().unwrap();
        let result = manager.load("nonexistent");
        assert!(matches!(result, Err(PluginError::NotFound(_))));
    }

    #[test]
    fn invoke_action() {
        let mut manager = PluginManager::new().unwrap();
        let manifest = test_manifest();
        let grants = GrantStore::new();
        manager.enable(manifest, grants).unwrap();
        manager.load("com.example.test").unwrap();

        let result = manager.invoke_action("com.example.test", "count", "{}");
        assert!(result.is_ok());
    }

    #[test]
    fn invoke_action_not_running() {
        let mut manager = PluginManager::new().unwrap();
        let manifest = test_manifest();
        let grants = GrantStore::new();
        manager.enable(manifest, grants).unwrap();
        // Don't load — still Enabled.

        let result = manager.invoke_action("com.example.test", "count", "{}");
        assert!(matches!(result, Err(PluginError::NotRunning(_))));
    }

    #[test]
    fn unload_plugin() {
        let mut manager = PluginManager::new().unwrap();
        let manifest = test_manifest();
        let grants = GrantStore::new();
        manager.enable(manifest, grants).unwrap();
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
        let grants = GrantStore::new();
        manager.enable(manifest, grants).unwrap();
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
        let grants = GrantStore::new();
        manager.enable(manifest, grants).unwrap();
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
        let grants = GrantStore::new();
        manager.enable(manifest, grants).unwrap();

        let ids = manager.plugin_ids();
        assert_eq!(ids, vec!["com.example.test"]);
    }

    #[test]
    fn has_capability() {
        let mut manager = PluginManager::new().unwrap();
        let manifest = test_manifest();
        let mut grants = GrantStore::new();
        grants.grant("com.example.test", Capability::ReadText);
        manager.enable(manifest, grants).unwrap();

        assert!(manager.has_capability("com.example.test", &Capability::ReadText));
        assert!(!manager.has_capability("com.example.test", &Capability::Annotate));
    }

    #[test]
    fn remove_plugin() {
        let mut manager = PluginManager::new().unwrap();
        let manifest = test_manifest();
        let grants = GrantStore::new();
        manager.enable(manifest, grants).unwrap();

        let removed = manager.remove("com.example.test");
        assert!(removed);
        assert!(manager.state("com.example.test").is_none());
    }
}
