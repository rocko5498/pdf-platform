//! WASM plugin host + Z0 control plane. [ADR-014, ADR-015, M11]
//!
//! This crate provides:
//! - Plugin manifest parsing and validation ([`manifest`])
//! - Capability grant model ([`grant`])
//! - Wasmtime WASM runtime ([`runtime`])
//! - Host function implementations ([`host_calls`])
//! - Plugin lifecycle manager ([`manager`])
//!
//! ## Architecture
//!
//! Per SDS §11 and ADR-014:
//! - **Control plane (Z0):** This crate manages plugin lifecycle, capability
//!   grants, and routes requests to the Broker.
//! - **Execution plane (Z2):** Plugin WASM instances run in utility workers
//!   under the OS sandbox, double-isolated (WASM inside OS sandbox).
//!
//! The execution plane will be wired when the utility worker pool is
//! implemented. For now, plugin instances can run in document workers.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod grant;
pub mod manifest;
pub mod runtime;
pub mod host_calls;
pub mod manager;

// Re-export key types for convenience.
pub use manifest::{PluginManifest, Capability, ManifestError};
pub use grant::{CapabilityGrant, GrantStore};
pub use runtime::{PluginRuntime, InstanceConfig, RuntimeError};
pub use manager::{PluginManager, PluginError, PluginUI};

/// Plugin lifecycle state. [SDS §11]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginState {
    /// Manifest discovered and validated.
    Discovered,
    /// Plugin enabled by user/admin; grants recorded.
    Enabled,
    /// WASM instance loaded and running.
    Running,
    /// Plugin failed to load or crashed.
    Failed {
        /// Human-readable failure reason. [GR-8]
        reason: String,
    },
    /// Plugin disabled by user/admin.
    Disabled,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_state_transitions() {
        let state = PluginState::Discovered;
        assert_ne!(state, PluginState::Running);

        let failed = PluginState::Failed {
            reason: "quota breach".into(),
        };
        match &failed {
            PluginState::Failed { reason } => assert_eq!(reason, "quota breach"),
            _ => panic!("expected Failed"),
        }
    }

    #[test]
    fn re_exports_work() {
        // Verify re-exported types are accessible.
        let _ = std::any::TypeId::of::<PluginManifest>();
        let _ = std::any::TypeId::of::<GrantStore>();
        let _ = std::any::TypeId::of::<PluginRuntime>();
        let _ = std::any::TypeId::of::<PluginManager>();
    }
}
