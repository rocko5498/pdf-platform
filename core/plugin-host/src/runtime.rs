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
}

/// The WASM plugin runtime. [SDS §2.7]
///
/// Owns the wasmtime Engine (shared across instances) and provides
/// methods to compile, instantiate, and manage plugin instances.
pub struct PluginRuntime {
    engine: Engine,
}

impl PluginRuntime {
    /// Create a new plugin runtime with default configuration.
    pub fn new() -> Result<Self, RuntimeError> {
        let engine = Engine::default();
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
        };
        Store::new(&self.engine, state)
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
