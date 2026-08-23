//! Wasmtime WASM plugin runtime. [ADR-014, ADR-015, M11]
//!
//! Manages the wasmtime engine, stores, and plugin instances.
//! CPU quotas and memory limits are tracked per-instance.

use std::collections::HashMap;

use wasmtime::{Engine, Store, Module};

use crate::grant::GrantStore;
use crate::manifest::PluginManifest;

/// Configuration for a plugin instance's resource limits.
#[derive(Debug, Clone)]
pub struct InstanceConfig {
    /// Maximum fuel units (CPU quota). Exceeding this preempts the instance.
    pub fuel_limit: u64,
    /// Maximum memory in bytes.
    pub memory_limit: usize,
}

impl Default for InstanceConfig {
    fn default() -> Self {
        Self {
            fuel_limit: 1_000_000,        // 1M fuel units
            memory_limit: 64 * 1024 * 1024, // 64 MiB
        }
    }
}

/// Per-instance state held in the wasmtime Store.
pub struct PluginInstanceState {
    /// The plugin's manifest.
    pub manifest: PluginManifest,
    /// Capability grants for this plugin.
    pub grants: GrantStore,
    /// Fuel budget remaining.
    pub fuel_remaining: u64,
    /// Fuel consumed so far.
    pub fuel_consumed: u64,
    /// Whether the instance has been initialized (init called).
    pub initialized: bool,
    /// Whether the instance has been shut down.
    pub shut_down: bool,
    /// Page count from the coordinator (cached for host functions).
    pub page_count: u32,
    /// Page text cache (page_index -> text) for host functions.
    pub page_texts: HashMap<u32, String>,
    /// Next handle ID for shared result store.
    pub next_handle: u32,
    /// Memory ceiling enforced by wasmtime for this instance. [GR-7]
    pub limits: PluginResourceLimits,
}

/// Enforces the instance's memory ceiling inside wasmtime.
///
/// `InstanceConfig::memory_limit` was documented as "maximum memory in bytes"
/// and never given to the engine, so it bounded nothing at all.
#[derive(Debug)]
pub struct PluginResourceLimits {
    /// Maximum linear memory, in bytes.
    pub memory_limit: usize,
}

impl wasmtime::ResourceLimiter for PluginResourceLimits {
    fn memory_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        Ok(desired <= self.memory_limit)
    }

    fn table_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        // A table entry is a reference; this bounds table growth to something
        // a plugin has no legitimate reason to exceed.
        Ok(desired <= 10_000)
    }
}

/// The WASM plugin runtime. [SDS §2.7]
///
/// Owns the wasmtime Engine (shared across instances) and provides
/// methods to compile, instantiate, and manage plugin instances.
pub struct PluginRuntime {
    engine: Engine,
}

impl PluginRuntime {
    /// Create a new plugin runtime.
    ///
    /// The engine meters fuel. Without `consume_fuel`, `InstanceConfig`'s
    /// `fuel_limit` was a number the host decremented by hand while the guest
    /// ran unmetered — a plugin loop would never be preempted, whatever the
    /// configured "CPU quota" said. [ADR-014, GR-7, PRIN-6]
    pub fn new() -> Result<Self, RuntimeError> {
        let mut config = wasmtime::Config::new();
        config.consume_fuel(true);
        let engine =
            Engine::new(&config).map_err(|e| RuntimeError::Compile(e.to_string()))?;
        Ok(Self { engine })
    }

    /// Get a reference to the wasmtime Engine.
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Compile a WASM binary into a Module.
    pub fn compile(&self, wasm_bytes: &[u8]) -> Result<Module, RuntimeError> {
        Module::new(&self.engine, wasm_bytes)
            .map_err(|e| RuntimeError::Compile(e.to_string()))
    }

    /// Create a new Store for a plugin instance with the given config.
    pub fn create_store(
        &self,
        manifest: PluginManifest,
        grants: GrantStore,
        config: InstanceConfig,
    ) -> Store<PluginInstanceState> {
        let state = PluginInstanceState {
            manifest,
            grants,
            fuel_remaining: config.fuel_limit,
            fuel_consumed: 0,
            initialized: false,
            shut_down: false,
            page_count: 0,
            page_texts: HashMap::new(),
            next_handle: 1,
            limits: PluginResourceLimits { memory_limit: config.memory_limit },
        };
        let mut store = Store::new(&self.engine, state);
        // Give the guest the fuel the config promised, and bound the memory it
        // may grow into. Both numbers existed before; neither reached wasmtime,
        // so a guest could spin forever and allocate until the host died.
        // [ADR-014, GR-7]
        store.set_fuel(config.fuel_limit).expect("engine is configured to consume fuel");
        store.limiter(|state| &mut state.limits);
        store
    }

    /// Consume fuel from a store, returning the amount consumed.
    ///
    /// Returns `None` if the store has no fuel remaining (preempted).
    pub fn consume_fuel(
        &self,
        store: &mut Store<PluginInstanceState>,
        amount: u64,
    ) -> Option<u64> {
        let data = store.data_mut();
        if amount > data.fuel_remaining {
            // Would exceed quota — preempt.
            data.fuel_consumed += data.fuel_remaining;
            data.fuel_remaining = 0;
            None
        } else {
            data.fuel_remaining -= amount;
            data.fuel_consumed += amount;
            Some(amount)
        }
    }

    /// Check if a store has been preempted (fuel exhausted).
    pub fn is_preempted(&self, store: &Store<PluginInstanceState>) -> bool {
        store.data().fuel_remaining == 0
    }

    /// Get fuel consumed by a store.
    pub fn fuel_consumed(&self, store: &Store<PluginInstanceState>) -> u64 {
        store.data().fuel_consumed
    }

    /// Get fuel remaining in a store.
    pub fn fuel_remaining(&self, store: &Store<PluginInstanceState>) -> u64 {
        store.data().fuel_remaining
    }
}

/// Errors from the plugin runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeError {
    /// Engine initialization failed.
    EngineInit(String),
    /// WASM compilation failed.
    Compile(String),
    /// Instantiation failed.
    Instantiate(String),
    /// A host function call failed.
    HostCall(String),
    /// The instance was preempted (fuel exhausted).
    Preempted,
    /// The instance exceeded its memory limit.
    MemoryExceeded,
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EngineInit(msg) => write!(f, "engine init failed: {msg}"),
            Self::Compile(msg) => write!(f, "WASM compile failed: {msg}"),
            Self::Instantiate(msg) => write!(f, "instantiation failed: {msg}"),
            Self::HostCall(msg) => write!(f, "host call failed: {msg}"),
            Self::Preempted => write!(f, "instance preempted (fuel exhausted)"),
            Self::MemoryExceeded => write!(f, "instance exceeded memory limit"),
        }
    }
}

impl std::error::Error for RuntimeError {}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal manifest for runtime tests.
    fn limits_test_manifest() -> PluginManifest {
        PluginManifest {
            id: "limits".into(),
            name: "Limits".into(),
            version: "1.0.0".into(),
            author: "A".into(),
            description: "D".into(),
            wit_world: "pdf-platform:plugin@1".into(),
            capabilities: vec![],
            panels: vec![],
            tools: vec![],
            job_types: vec![],
        }
    }

    /// A guest that never returns must be stopped by the engine.
    ///
    /// Before fuel was actually metered this test could not be written: the
    /// call would simply hang, which is what a malicious or buggy plugin would
    /// do to the application. [ADR-014, GR-7]
    ///
    /// Not run on Windows. There, exhausting fuel aborts the process with
    /// "panic in a function that cannot unwind" from
    /// `wasmtime::runtime::vm::libcalls::raw::out_of_gas` instead of returning
    /// a trap — reproduced with plain wasmtime 28 and no code of ours, so it is
    /// an upstream or toolchain problem, not this crate's. It is recorded in
    /// the tracker as an open defect rather than hidden: on Windows a runaway
    /// plugin currently takes the application down with it, which is worse than
    /// the unmetered loop it replaces. [ADR-014, GR-7, PRIN-6]
    #[cfg(not(windows))]
    #[test]
    fn an_endless_plugin_loop_is_preempted() {
        let runtime = PluginRuntime::new().expect("runtime");
        let module = wasmtime::Module::new(
            runtime.engine(),
            r#"(module (func (export "spin") (loop br 0)))"#,
        )
        .expect("compile spin module");

        let mut store = runtime.create_store(
            limits_test_manifest(),
            GrantStore::new(),
            InstanceConfig { fuel_limit: 10_000, memory_limit: 1 << 20 },
        );
        let instance = wasmtime::Instance::new(&mut store, &module, &[]).expect("instantiate");
        let spin = instance
            .get_typed_func::<(), ()>(&mut store, "spin")
            .expect("spin export");

        let outcome = spin.call(&mut store, ());
        assert!(
            outcome.is_err(),
            "an endless loop returned normally, so nothing preempted it"
        );
        assert_eq!(
            store.get_fuel().expect("fuel is metered"),
            0,
            "the trap must be fuel exhaustion, not something incidental"
        );
    }

    /// The memory ceiling must be refused by the engine, not merely recorded.
    #[test]
    fn a_plugin_cannot_grow_past_its_memory_limit() {
        let runtime = PluginRuntime::new().expect("runtime");
        // One page (64 KiB) initial, grows on demand.
        let module = wasmtime::Module::new(
            runtime.engine(),
            r#"(module
                 (memory (export "mem") 1)
                 (func (export "grow") (param i32) (result i32)
                   (memory.grow (local.get 0))))"#,
        )
        .expect("compile memory module");

        let mut store = runtime.create_store(
            limits_test_manifest(),
            GrantStore::new(),
            // Two pages: the initial one plus room for exactly one more.
            InstanceConfig { fuel_limit: 1_000_000, memory_limit: 128 * 1024 },
        );
        let instance = wasmtime::Instance::new(&mut store, &module, &[]).expect("instantiate");
        let grow = instance
            .get_typed_func::<i32, i32>(&mut store, "grow")
            .expect("grow export");

        // One more page fits.
        assert_eq!(grow.call(&mut store, 1).expect("grow by one page"), 1);
        // Ten more do not: memory.grow answers -1 when the limiter refuses.
        assert_eq!(
            grow.call(&mut store, 10).expect("grow call itself succeeds"),
            -1,
            "the limiter allowed growth past the configured ceiling"
        );
    }

    #[test]
    fn default_instance_config() {
        let config = InstanceConfig::default();
        assert_eq!(config.fuel_limit, 1_000_000);
        assert_eq!(config.memory_limit, 64 * 1024 * 1024);
    }

    #[test]
    fn runtime_creation() {
        let runtime = PluginRuntime::new();
        assert!(runtime.is_ok());
    }

    #[test]
    fn compile_empty_module() {
        let runtime = PluginRuntime::new().unwrap();
        // A minimal valid WASM module (empty module).
        let wasm = wat::parse_str("(module)").unwrap();
        let result = runtime.compile(&wasm);
        assert!(result.is_ok());
    }

    #[test]
    fn compile_invalid_wasm() {
        let runtime = PluginRuntime::new().unwrap();
        let result = runtime.compile(b"not wasm");
        assert!(result.is_err());
        assert!(matches!(result, Err(RuntimeError::Compile(_))));
    }

    #[test]
    fn store_creation() {
        let runtime = PluginRuntime::new().unwrap();
        let manifest = PluginManifest {
            id: "test".into(),
            name: "Test".into(),
            version: "1.0.0".into(),
            author: "A".into(),
            description: "D".into(),
            wit_world: "pdf-platform:plugin@1".into(),
            capabilities: vec![],
            panels: vec![],
            tools: vec![],
            job_types: vec![],
        };
        let config = InstanceConfig::default();
        let grants = GrantStore::new();
        let store = runtime.create_store(manifest, grants, config);
        assert_eq!(store.data().fuel_consumed, 0);
        assert!(!store.data().initialized);
    }

    #[test]
    fn fuel_consume_and_check() {
        let runtime = PluginRuntime::new().unwrap();
        let manifest = PluginManifest {
            id: "test".into(),
            name: "Test".into(),
            version: "1.0.0".into(),
            author: "A".into(),
            description: "D".into(),
            wit_world: "pdf-platform:plugin@1".into(),
            capabilities: vec![],
            panels: vec![],
            tools: vec![],
            job_types: vec![],
        };
        let config = InstanceConfig {
            fuel_limit: 100,
            memory_limit: 1024,
        };
        let grants = GrantStore::new();
        let mut store = runtime.create_store(manifest, grants, config);

        // Consume some fuel.
        let consumed = runtime.consume_fuel(&mut store, 50);
        assert_eq!(consumed, Some(50));
        assert_eq!(runtime.fuel_remaining(&store), 50);
        assert_eq!(runtime.fuel_consumed(&store), 50);

        // Consume more.
        let consumed = runtime.consume_fuel(&mut store, 30);
        assert_eq!(consumed, Some(30));
        assert_eq!(runtime.fuel_remaining(&store), 20);
        assert_eq!(runtime.fuel_consumed(&store), 80);

        // Try to consume more than remaining — should preempt.
        let consumed = runtime.consume_fuel(&mut store, 30);
        assert_eq!(consumed, None);
        assert!(runtime.is_preempted(&store));
        assert_eq!(runtime.fuel_remaining(&store), 0);
        assert_eq!(runtime.fuel_consumed(&store), 100);
    }

    #[test]
    fn runtime_error_display() {
        let err = RuntimeError::Preempted;
        assert_eq!(err.to_string(), "instance preempted (fuel exhausted)");

        let err = RuntimeError::Compile("bad bytes".into());
        assert!(err.to_string().contains("bad bytes"));
    }
}
