//! Capability grant model. [ADR-014 §2, FR-PLUG-2, M11]
//!
//! Plugins declare needed capabilities in their manifest; users (or admins)
//! grant per-capability at enable time. Grants are revocable. A plugin can
//! never exceed its grant because ungranted host functions are absent from
//! its WASM instance. [ADR-014 §2]

use std::collections::HashMap;
use std::time::SystemTime;

use crate::manifest::Capability;

/// A single capability grant for a plugin.
#[derive(Debug, Clone, PartialEq)]
pub struct CapabilityGrant {
    /// The plugin this grant belongs to.
    pub plugin_id: String,
    /// The capability granted.
    pub capability: Capability,
    /// Whether this grant is currently active.
    pub granted: bool,
    /// When the grant was made (None if revoked).
    pub granted_at: Option<SystemTime>,
}

/// Storage for all plugin capability grants. [SDS §11.3]
///
/// The grant store is the Z0 control-plane authority for what each plugin
/// may do. It is consulted before routing any plugin request.
#[derive(Debug, Clone, Default)]
pub struct GrantStore {
    grants: HashMap<String, Vec<CapabilityGrant>>,
}

impl GrantStore {
    /// Create an empty grant store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Grant a capability to a plugin.
    pub fn grant(&mut self, plugin_id: &str, capability: Capability) {
        let entry = self.grants.entry(plugin_id.to_string()).or_default();
        // Check if this capability already has a grant
        if let Some(existing) = entry
            .iter_mut()
            .find(|g| g.capability == capability)
        {
            existing.granted = true;
            existing.granted_at = Some(SystemTime::now());
        } else {
            entry.push(CapabilityGrant {
                plugin_id: plugin_id.to_string(),
                capability,
                granted: true,
                granted_at: Some(SystemTime::now()),
            });
        }
    }

    /// Revoke a capability from a plugin.
    pub fn revoke(&mut self, plugin_id: &str, capability: &Capability) {
        if let Some(entry) = self.grants.get_mut(plugin_id) {
            if let Some(grant) = entry.iter_mut().find(|g| &g.capability == capability) {
                grant.granted = false;
                grant.granted_at = None;
            }
        }
    }

    /// Check if a plugin has a specific capability granted.
    pub fn is_granted(&self, plugin_id: &str, capability: &Capability) -> bool {
        self.grants
            .get(plugin_id)
            .and_then(|entry| entry.iter().find(|g| &g.capability == capability))
            .is_some_and(|g| g.granted)
    }

    /// Get all grants for a plugin.
    pub fn grants_for(&self, plugin_id: &str) -> &[CapabilityGrant] {
        self.grants
            .get(plugin_id)
            .map_or(&[], |v| v.as_slice())
    }

    /// Remove all grants for a plugin (e.g., on uninstall).
    pub fn remove_all(&mut self, plugin_id: &str) {
        self.grants.remove(plugin_id);
    }

    /// Number of plugins with grants.
    pub fn plugin_count(&self) -> usize {
        self.grants.len()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grant_and_check() {
        let mut store = GrantStore::new();
        store.grant("plugin-a", Capability::ReadText);
        assert!(store.is_granted("plugin-a", &Capability::ReadText));
        assert!(!store.is_granted("plugin-a", &Capability::Annotate));
        assert!(!store.is_granted("plugin-b", &Capability::ReadText));
    }

    #[test]
    fn revoke() {
        let mut store = GrantStore::new();
        store.grant("p", Capability::ReadText);
        assert!(store.is_granted("p", &Capability::ReadText));
        store.revoke("p", &Capability::ReadText);
        assert!(!store.is_granted("p", &Capability::ReadText));
    }

    #[test]
    fn re_grant_overwrites() {
        let mut store = GrantStore::new();
        store.grant("p", Capability::ReadText);
        store.grant("p", Capability::ReadText); // idempotent
        assert!(store.is_granted("p", &Capability::ReadText));
        let grants = store.grants_for("p");
        assert_eq!(grants.len(), 1);
    }

    #[test]
    fn remove_all() {
        let mut store = GrantStore::new();
        store.grant("p", Capability::ReadText);
        store.grant("p", Capability::Annotate);
        store.remove_all("p");
        assert!(!store.is_granted("p", &Capability::ReadText));
        assert_eq!(store.plugin_count(), 0);
    }

    #[test]
    fn grants_for_returns_empty_for_unknown() {
        let store = GrantStore::new();
        assert!(store.grants_for("nonexistent").is_empty());
    }

    #[test]
    fn multiple_capabilities() {
        let mut store = GrantStore::new();
        store.grant("p", Capability::ReadText);
        store.grant("p", Capability::ReadStructure);
        store.grant("p", Capability::Annotate);
        assert!(store.is_granted("p", &Capability::ReadText));
        assert!(store.is_granted("p", &Capability::ReadStructure));
        assert!(store.is_granted("p", &Capability::Annotate));
        assert_eq!(store.grants_for("p").len(), 3);
    }
}
