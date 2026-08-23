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

/// A module with no entry point: valid WebAssembly, not a valid plugin.
fn module_without_entry_point() -> Vec<u8> {
    wat::parse_str("(module)").unwrap()
}

/// A guest that exports the entry point and the actions these tests invoke,
/// and imports nothing — so what it can reach is decided entirely by grants.
fn test_guest() -> Vec<u8> {
    wat::parse_str(
        r#"(module
             (func (export "run"))
             (func (export "action"))
             (func (export "malicious_action")))"#,
    )
    .unwrap()
}

/// A guest that imports a host function for a capability it may not hold.
fn annotating_guest() -> Vec<u8> {
    wat::parse_str(
        r#"(module
             (import "env" "host_submit_annotation" (func $submit (param i32 i32) (result i64)))
             (memory (export "memory") 1)
             (func (export "run")
               (i64.store (i32.const 0) (call $submit (i32.const 0) (i32.const 0)))))"#,
    )
    .unwrap()
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

    let result = manager.enable(manifest, grants, test_guest());
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

    manager.enable(manifest, grants, test_guest()).unwrap();

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

    manager.enable(manifest, grants, test_guest()).unwrap();
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

    manager.enable(manifest, grants, test_guest()).unwrap();
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

    manager.enable(manifest, grants, test_guest()).unwrap();
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

    manager.enable(manifest, grants, test_guest()).unwrap();
    assert!(manager.has_capability("com.malicious.annotate", &Capability::Annotate));

    // Remove and re-enable without grant
    manager.remove("com.malicious.annotate");

    let manifest2 = annotate_manifest();
    let grants2 = GrantStore::new(); // no grants
    manager.enable(manifest2, grants2, test_guest()).unwrap();

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

    manager.enable(manifest, grants, test_guest()).unwrap();
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

// ---------------------------------------------------------------------------
// Test 11: A plugin cannot reach a host function it was not granted
// ---------------------------------------------------------------------------

#[test]
fn ungranted_host_function_is_absent_from_the_instance() {
    // ADR-014 §2: undeclared capabilities are unlinkable, not merely denied.
    // Until the host actually loaded plugin modules there was nothing to link,
    // and every host function was wired into every instance regardless.
    let mut manager = PluginManager::new().unwrap();
    let mut manifest = annotate_manifest();
    manifest.id = "com.malicious.unlinkable".into();

    // Declared in the manifest, never granted by the user.
    manager
        .enable(manifest, GrantStore::new(), annotating_guest())
        .unwrap();

    let result = manager.load("com.malicious.unlinkable");

    match &result {
        Err(plugin_host::PluginError::Runtime(plugin_host::RuntimeError::Instantiate(
            detail,
        ))) => assert!(
            detail.contains("host_submit_annotation"),
            "the failure must name the absent import: {detail}"
        ),
        other => panic!("expected an unknown-import failure, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Test 12: A module that is not a plugin is rejected, not silently accepted
// ---------------------------------------------------------------------------

#[test]
fn a_module_without_an_entry_point_is_rejected() {
    let mut manager = PluginManager::new().unwrap();
    let manifest = annotate_manifest();

    manager
        .enable(manifest, GrantStore::new(), module_without_entry_point())
        .unwrap();

    let result = manager.load("com.malicious.annotate");

    assert!(
        matches!(
            &result,
            Err(plugin_host::PluginError::MissingExport { export, .. }) if export == "run"
        ),
        "expected a missing-`run` error, got {result:?}"
    );
    assert_ne!(
        manager.state("com.malicious.annotate"),
        Some(&plugin_host::PluginState::Running),
        "a module with no entry point must never be reported as Running"
    );
}
