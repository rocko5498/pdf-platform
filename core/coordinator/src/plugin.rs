//! Coordinator-side plugin host. [SDS §2.2.7, ADR-014, M11]
//!
//! The CoordinatorPluginHost is the Z0 control plane for plugins.
//! It wraps the plugin-host crate's PluginManager and routes plugin
//! requests through the capability broker.

use plugin_host::{
    GrantStore, PluginError, PluginManager, PluginManifest, PluginState, PluginUI,
    manifest::Capability,
};

/// Coordinator-side plugin host. [SDS §2.2.7]
///
/// Owns the PluginManager and provides the interface that the
/// DocumentCoordinator uses to manage plugins.
pub struct CoordinatorPluginHost {
    /// The underlying plugin manager.
    manager: PluginManager,
}

impl CoordinatorPluginHost {
    /// Create a new coordinator plugin host.
    pub fn new() -> Result<Self, plugin_host::RuntimeError> {
        let manager = PluginManager::new()?;
        Ok(Self { manager })
    }

    /// Discover and validate a plugin from JSON manifest bytes.
    pub fn discover(&self, manifest_bytes: &[u8]) -> Result<PluginManifest, PluginError> {
        self.manager.discover(manifest_bytes)
    }

    /// Enable a plugin with the given capability grants and its module bytes.
    ///
    /// `module_wasm` comes from the plugin package alongside the manifest
    /// (`SDS §11.1`). The plugin is instantiated on first use, not here.
    pub fn enable(
        &mut self,
        manifest: PluginManifest,
        grants: GrantStore,
        module_wasm: Vec<u8>,
    ) -> Result<(), PluginError> {
        self.manager.enable(manifest, grants, module_wasm)
    }

    /// Load (instantiate) a plugin by ID.
    pub fn load(&mut self, plugin_id: &str) -> Result<(), PluginError> {
        self.manager.load(plugin_id)
    }

    /// Unload a plugin by ID.
    pub fn unload(&mut self, plugin_id: &str) -> Result<(), PluginError> {
        self.manager.unload(plugin_id)
    }

    /// Invoke a plugin-registered tool action.
    pub fn invoke_action(
        &mut self,
        plugin_id: &str,
        action_id: &str,
        args_json: &str,
    ) -> Result<String, PluginError> {
        self.manager.invoke_action(plugin_id, action_id, args_json)
    }

    /// Get the current state of a plugin.
    pub fn state(&self, plugin_id: &str) -> Option<&PluginState> {
        self.manager.state(plugin_id)
    }

    /// Get UI contributions from a plugin.
    pub fn ui(&self, plugin_id: &str) -> Option<&PluginUI> {
        self.manager.ui(plugin_id)
    }

    /// Get all loaded plugin IDs.
    pub fn plugin_ids(&self) -> Vec<&str> {
        self.manager.plugin_ids()
    }

    /// Check if a plugin has a specific capability granted.
    pub fn has_capability(&self, plugin_id: &str, capability: &Capability) -> bool {
        self.manager.has_capability(plugin_id, capability)
    }

    /// Record a failure for the circuit breaker.
    pub fn record_failure(&mut self, plugin_id: &str) {
        self.manager.record_failure(plugin_id);
    }

    /// Record a success (resets the failure count).
    pub fn record_success(&mut self, plugin_id: &str) {
        self.manager.record_success(plugin_id);
    }

    /// Remove a plugin entirely (e.g., on uninstall).
    pub fn remove(&mut self, plugin_id: &str) {
        self.manager.remove(plugin_id);
    }

    /// Get a reference to the underlying PluginManager.
    pub fn manager(&self) -> &PluginManager {
        &self.manager
    }

    /// Get a mutable reference to the underlying PluginManager.
    pub fn manager_mut(&mut self) -> &mut PluginManager {
        &mut self.manager
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use plugin_host::manifest::{
        PanelContribution, ToolContribution, JobTypeContribution,
    };

    fn test_manifest() -> PluginManifest {
        PluginManifest {
            id: "com.example.coordinator".into(),
            name: "Coordinator Test".into(),
            version: "1.0.0".into(),
            author: "A".into(),
            description: "D".into(),
            wit_world: "pdf-platform:plugin@1".into(),
            capabilities: vec![Capability::ReadText],
            panels: vec![],
            tools: vec![],
            job_types: vec![],
        }
    }

    fn manifest_json() -> String {
        r#"{
            "id": "com.example.coordinator",
            "name": "Coordinator Test",
            "version": "1.0.0",
            "author": "A",
            "description": "D",
            "wit_world": "pdf-platform:plugin@1",
            "capabilities": [{"type": "ReadText"}]
        }"#
        .into()
    }

    /// A guest exporting the entry point and the action these tests invoke.
    /// It imports nothing, so it links whatever the grants are.
    fn test_guest() -> Vec<u8> {
        br#"(module (func (export "run")) (func (export "test")))"#.to_vec()
    }

    #[test]
    fn create_plugin_host() {
        let host = CoordinatorPluginHost::new();
        assert!(host.is_ok());
    }

    #[test]
    fn discover_manifest() {
        let host = CoordinatorPluginHost::new().unwrap();
        let manifest = host.discover(manifest_json().as_bytes()).unwrap();
        assert_eq!(manifest.id, "com.example.coordinator");
    }

    #[test]
    fn enable_and_load() {
        let mut host = CoordinatorPluginHost::new().unwrap();
        let manifest = test_manifest();
        let grants = GrantStore::new();
        host.enable(manifest, grants, test_guest()).unwrap();
        host.load("com.example.coordinator").unwrap();
        assert_eq!(
            host.state("com.example.coordinator"),
            Some(&PluginState::Running)
        );
    }

    #[test]
    fn invoke_action_reaches_the_guest_through_the_coordinator() {
        // `is_ok()` was true while the manager returned a canned JSON string
        // without calling anything. An action the guest does not export is the
        // observable that separates dispatch from decoration. [T-10, T-12]
        let mut host = CoordinatorPluginHost::new().unwrap();
        host.enable(test_manifest(), GrantStore::new(), test_guest())
            .unwrap();
        host.load("com.example.coordinator").unwrap();

        host.invoke_action("com.example.coordinator", "test", "{}")
            .expect("the guest exports `test`");

        let missing = host.invoke_action("com.example.coordinator", "absent", "{}");
        assert!(
            matches!(&missing, Err(PluginError::MissingExport { export, .. }) if export == "absent"),
            "expected a missing-export error, got {missing:?}"
        );
    }

    #[test]
    fn unload_plugin() {
        let mut host = CoordinatorPluginHost::new().unwrap();
        let manifest = test_manifest();
        let grants = GrantStore::new();
        host.enable(manifest, grants, test_guest()).unwrap();
        host.load("com.example.coordinator").unwrap();
        host.unload("com.example.coordinator").unwrap();

        assert_eq!(
            host.state("com.example.coordinator"),
            Some(&PluginState::Disabled)
        );
    }

    #[test]
    fn plugin_ids() {
        let mut host = CoordinatorPluginHost::new().unwrap();
        let manifest = test_manifest();
        let grants = GrantStore::new();
        host.enable(manifest, grants, test_guest()).unwrap();

        let ids = host.plugin_ids();
        assert_eq!(ids, vec!["com.example.coordinator"]);
    }

    #[test]
    fn has_capability() {
        let mut host = CoordinatorPluginHost::new().unwrap();
        let manifest = test_manifest();
        let mut grants = GrantStore::new();
        grants.grant("com.example.coordinator", Capability::ReadText);
        host.enable(manifest, grants, test_guest()).unwrap();

        assert!(host.has_capability("com.example.coordinator", &Capability::ReadText));
        assert!(!host.has_capability("com.example.coordinator", &Capability::Annotate));
    }

    #[test]
    fn remove_plugin() {
        let mut host = CoordinatorPluginHost::new().unwrap();
        let manifest = test_manifest();
        let grants = GrantStore::new();
        host.enable(manifest, grants, test_guest()).unwrap();

        host.remove("com.example.coordinator");
        assert!(host.state("com.example.coordinator").is_none());
    }

    #[test]
    fn circuit_breaker() {
        let mut host = CoordinatorPluginHost::new().unwrap();
        let manifest = test_manifest();
        let grants = GrantStore::new();
        host.enable(manifest, grants, test_guest()).unwrap();
        host.load("com.example.coordinator").unwrap();

        // Record 3 failures to trip the breaker.
        for _ in 0..3 {
            host.record_failure("com.example.coordinator");
        }

        let result = host.invoke_action("com.example.coordinator", "test", "{}");
        assert!(result.is_err());
    }
}
