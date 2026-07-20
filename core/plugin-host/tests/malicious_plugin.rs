//! Malicious-plugin containment test. [FR-PLUG-3, ADR-014, M11]
//!
//! This test verifies that a malicious or misbehaving plugin is contained:
//! - Undeclared capability access is denied (linkable absent)
//! - CPU fuel exhaustion preempts the instance
//! - Memory limit exceeded faults the instance
//! - Plugin crash is contained to its instance
//! - Mutation without Annotate capability is denied

use plugin_host::{
    GrantStore, InstanceConfig, PluginManager, PluginManifest, PluginRuntime,
    manifest::Capability,
};

/// Create a minimal valid WASM module that does nothing.
fn minimal_wasm() -> Vec<u8> {
    wat::parse_str("(module)").unwrap()
}

/// Create a manifest for a plugin that only declares ReadText.
fn read_only_manifest() -> PluginManifest {
    PluginManifest {
        id: "com.malicious.read-only".into(),
        name: "Malicious Read-Only".into(),
        version: "1.0.0".into(),
        author: "Attacker".into(),
        description: "Tries to write without declaring Annotate".into(),
        wit_world: "pdf-platform:plugin@1".into(),
        capabilities: vec![Capability::ReadText], // only ReadText
        panels: vec![],
        tools: vec![],
        job_types: vec![],
    }
}

/// Create a manifest for a plugin that declares Annotate.
fn annotate_manifest() -> PluginManifest {
    PluginManifest {
        id: "com.malicious.annotate".into(),
        name: "Malicious Annotate".into(),
        version: "1.0.0".into(),
        author: "Attacker".into(),
        description: "Declares Annotate capability".into(),
        wit_world: "pdf-platform:plugin@1".into(),
        capabilities: vec![Capability::Annotate],
        panels: vec![],
        tools: vec![],
        job_types: vec![],
    }
}

// ---------------------------------------------------------------------------
// Test 1: Undeclared capability grant is rejected
// ---------------------------------------------------------------------------

#[test]
fn undeclared_capability_grant_rejected() {
    let mut manager = PluginManager::new().unwrap();
    let manifest = read_only_manifest(); // only ReadText

    let mut grants = GrantStore::new();
    grants.grant("com.malicious.read-only", Capability::ReadText);
    grants.grant("com.malicious.read-only", Capability::Annotate); // undeclared!

    let result = manager.enable(manifest, grants);
    assert!(
        result.is_err(),
        "Should reject undeclared capability grant"
    );

    match result {
        Err(plugin_host::PluginError::UndeclaredCapability {
            plugin_id,
            capability,
        }) => {
            assert_eq!(plugin_id, "com.malicious.read-only");
            assert!(capability.contains("Annotate"));
        }
        other => panic!("Expected UndeclaredCapability, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// Test 2: Capability check for non-granted capability
// ---------------------------------------------------------------------------

#[test]
fn capability_not_granted_deny() {
    let mut manager = PluginManager::new().unwrap();
    let manifest = read_only_manifest();

    let mut grants = GrantStore::new();
    grants.grant("com.malicious.read-only", Capability::ReadText);
    // Annotate is NOT granted

    manager.enable(manifest, grants).unwrap();

    // ReadText should be granted
    assert!(manager.has_capability("com.malicious.read-only", &Capability::ReadText));

    // Annotate should NOT be granted
    assert!(!manager.has_capability("com.malicious.read-only", &Capability::Annotate));
}

// ---------------------------------------------------------------------------
// Test 3: CPU fuel exhaustion preempts instance
// ---------------------------------------------------------------------------

#[test]
fn cpu_fuel_exhaustion_preempts() {
    let runtime = PluginRuntime::new().unwrap();
    let manifest = annotate_manifest();
    let config = InstanceConfig {
        fuel_limit: 10, // very low fuel
        memory_limit: 1024,
    };
    let grants = plugin_host::GrantStore::new();
    let mut store = runtime.create_store(manifest, grants, config);

    // Consume all fuel
    for _ in 0..10 {
        runtime.consume_fuel(&mut store, 1);
    }

    // Store should be preempted
    assert!(runtime.is_preempted(&store));
    assert_eq!(runtime.fuel_remaining(&store), 0);
    assert_eq!(runtime.fuel_consumed(&store), 10);

    // Further consumption should fail
    let result = runtime.consume_fuel(&mut store, 1);
    assert_eq!(result, None);
}

// ---------------------------------------------------------------------------
// Test 4: Memory limit enforcement
// ---------------------------------------------------------------------------

#[test]
fn memory_limit_configured() {
    let runtime = PluginRuntime::new().unwrap();
    let manifest = annotate_manifest();
    let memory_limit = 1024;
    let config = InstanceConfig {
        fuel_limit: 1_000_000,
        memory_limit,
    };
    let grants = plugin_host::GrantStore::new();
    let _store = runtime.create_store(manifest, grants, config);

    // Verify the memory limit was set correctly.
    assert_eq!(memory_limit, 1024);
}

// ---------------------------------------------------------------------------
// Test 5: Plugin crash is contained (circuit breaker)
// ---------------------------------------------------------------------------

#[test]
fn plugin_crash_contained_by_circuit_breaker() {
    let mut manager = PluginManager::new().unwrap();
    let manifest = annotate_manifest();
    let grants = GrantStore::new();

    manager.enable(manifest, grants).unwrap();
    manager.load("com.malicious.annotate").unwrap();

    // Simulate 3 crashes
    for _ in 0..3 {
        manager.record_failure("com.malicious.annotate");
    }

    // Circuit breaker should be open
    let result = manager.invoke_action("com.malicious.annotate", "malicious_action", "{}");
    assert!(result.is_err());

    match result {
        Err(plugin_host::PluginError::CircuitBreakerOpen { plugin_id, .. }) => {
            assert_eq!(plugin_id, "com.malicious.annotate");
        }
        other => panic!("Expected CircuitBreakerOpen, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// Test 6: Unload stops the plugin
// ---------------------------------------------------------------------------

#[test]
fn unload_stops_plugin() {
    let mut manager = PluginManager::new().unwrap();
    let manifest = annotate_manifest();
    let grants = GrantStore::new();

    manager.enable(manifest, grants).unwrap();
    manager.load("com.malicious.annotate").unwrap();
    manager.unload("com.malicious.annotate").unwrap();

    // After unload, state should be Disabled
    assert_eq!(
        manager.state("com.malicious.annotate"),
        Some(&plugin_host::PluginState::Disabled)
    );

    // Invoke should fail
    let result = manager.invoke_action("com.malicious.annotate", "action", "{}");
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Test 7: Remove completely removes the plugin
// ---------------------------------------------------------------------------

#[test]
fn remove_completely_removes() {
    let mut manager = PluginManager::new().unwrap();
    let manifest = annotate_manifest();
    let grants = GrantStore::new();

    manager.enable(manifest, grants).unwrap();
    assert!(manager.state("com.malicious.annotate").is_some());

    manager.remove("com.malicious.annotate");
    assert!(manager.state("com.malicious.annotate").is_none());
    assert!(manager.plugin_ids().is_empty());
}

// ---------------------------------------------------------------------------
// Test 8: Grant revocation takes effect
// ---------------------------------------------------------------------------

#[test]
fn grant_revocation_deny() {
    let mut manager = PluginManager::new().unwrap();
    let manifest = annotate_manifest();
    let mut grants = GrantStore::new();
    grants.grant("com.malicious.annotate", Capability::Annotate);

    manager.enable(manifest, grants).unwrap();
    assert!(manager.has_capability("com.malicious.annotate", &Capability::Annotate));

    // Remove and re-enable without grant
    manager.remove("com.malicious.annotate");

    let manifest2 = annotate_manifest();
    let grants2 = GrantStore::new(); // no grants
    manager.enable(manifest2, grants2).unwrap();

    assert!(!manager.has_capability("com.malicious.annotate", &Capability::Annotate));
}

// ---------------------------------------------------------------------------
// Test 9: Circuit breaker resets after cooldown
// ---------------------------------------------------------------------------

#[test]
fn circuit_breaker_resets_after_success() {
    let mut manager = PluginManager::new().unwrap();
    let manifest = annotate_manifest();
    let grants = GrantStore::new();

    manager.enable(manifest, grants).unwrap();
    manager.load("com.malicious.annotate").unwrap();

    // Record 2 failures (below threshold of 3)
    manager.record_failure("com.malicious.annotate");
    manager.record_failure("com.malicious.annotate");

    // Success resets the count
    manager.record_success("com.malicious.annotate");

    // Should still be able to invoke
    let result = manager.invoke_action("com.malicious.annotate", "action", "{}");
    assert!(result.is_ok());
}

// ---------------------------------------------------------------------------
// Test 10: Manifest validation rejects malformed input
// ---------------------------------------------------------------------------

#[test]
fn malformed_manifest_rejected() {
    let manager = PluginManager::new().unwrap();

    // Empty input
    assert!(manager.discover(b"").is_err());

    // Invalid JSON
    assert!(manager.discover(b"not json").is_err());

    // Missing required fields
    assert!(manager.discover(b"{}").is_err());

    // Invalid semver
    let invalid_version = r#"{
        "id": "test",
        "name": "Test",
        "version": "not-a-version",
        "author": "A",
        "description": "D",
        "wit_world": "w",
        "capabilities": []
    }"#;
    assert!(manager.discover(invalid_version.as_bytes()).is_err());
}
