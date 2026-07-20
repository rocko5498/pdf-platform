//! Compatibility test kit for PDF Platform plugins. [FR-PLUG-6, ADR-015, M11]
//!
//! This test validates that a plugin manifest declares compatibility
//! with the current WIT world version. Plugin authors run this test
//! in their CI to verify their plugin targets a supported world.

/// The current WIT world version this SDK supports.
///
/// This must match the version in `plugin-sdk/wit/plugin.wit`.
/// When the WIT world is updated, this version is bumped per semver.
pub const CURRENT_WIT_WORLD_VERSION: &str = "1.0.0";

/// The minimum WIT world version this SDK is backward-compatible with.
///
/// Per ADR-030, deprecated interfaces ship alongside successors for
/// ≥ 2 release trains. This means the current SDK supports worlds
/// from this version onward.
pub const MINIMUM_WIT_WORLD_VERSION: &str = "1.0.0";

/// Parse a semver string into (major, minor, patch).
fn parse_semver(version: &str) -> Option<(u32, u32, u32)> {
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() < 3 {
        return None;
    }
    let major = parts[0].parse::<u32>().ok()?;
    let minor = parts[1].parse::<u32>().ok()?;
    let patch = parts[2].parse::<u32>().ok()?;
    Some((major, minor, patch))
}

/// Check if a plugin's declared WIT world version is compatible
/// with the current SDK version.
///
/// Returns `Ok(())` if compatible, or `Err(message)` with the reason.
pub fn assert_wit_compatible(plugin_wit_version: &str) -> Result<(), String> {
    let plugin = parse_semver(plugin_wit_version)
        .ok_or_else(|| format!("invalid semver: '{plugin_wit_version}'"))?;
    let current = parse_semver(CURRENT_WIT_WORLD_VERSION)
        .expect("CURRENT_WIT_WORLD_VERSION must be valid semver");
    let minimum = parse_semver(MINIMUM_WIT_WORLD_VERSION)
        .expect("MINIMUM_WIT_WORLD_VERSION must be valid semver");

    // Major version must match (semver: breaking changes)
    if plugin.0 != current.0 {
        return Err(format!(
            "major version mismatch: plugin targets {plugin_wit_version}, \
             SDK requires {}.x.x",
            current.0
        ));
    }

    // Plugin minor version must not exceed current (can't use newer features)
    if plugin.1 > current.1 {
        return Err(format!(
            "plugin targets newer minor version: {plugin_wit_version} > {}.{}.x",
            current.0, current.1
        ));
    }

    // Plugin version must be at least the minimum
    if plugin < minimum {
        return Err(format!(
            "plugin targets {plugin_wit_version}, minimum supported is {MINIMUM_WIT_WORLD_VERSION}"
        ));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn current_version_is_valid() {
    assert!(parse_semver(CURRENT_WIT_WORLD_VERSION).is_some());
}

#[test]
fn minimum_version_is_valid() {
    assert!(parse_semver(MINIMUM_WIT_WORLD_VERSION).is_some());
}

#[test]
fn minimum_lte_current() {
    let min = parse_semver(MINIMUM_WIT_WORLD_VERSION).unwrap();
    let cur = parse_semver(CURRENT_WIT_WORLD_VERSION).unwrap();
    assert!(min <= cur, "minimum must be <= current");
}

#[test]
fn compatible_same_version() {
    assert!(assert_wit_compatible(CURRENT_WIT_WORLD_VERSION).is_ok());
}

#[test]
fn compatible_patch_bump() {
    // Plugin targets 1.0.5, SDK is 1.0.0 — compatible (same minor)
    assert!(assert_wit_compatible("1.0.5").is_ok());
}

#[test]
fn incompatible_major_mismatch() {
    // Plugin targets 2.0.0, SDK is 1.0.0 — incompatible
    assert!(assert_wit_compatible("2.0.0").is_err());
}

#[test]
fn incompatible_newer_minor() {
    // Plugin targets 1.1.0, SDK is 1.0.0 — plugin uses newer features
    assert!(assert_wit_compatible("1.1.0").is_err());
}

#[test]
fn incompatible_below_minimum() {
    // Plugin targets 0.9.0, minimum is 1.0.0 — too old
    assert!(assert_wit_compatible("0.9.0").is_err());
}

#[test]
fn reject_invalid_semver() {
    assert!(assert_wit_compatible("not-a-version").is_err());
    assert!(assert_wit_compatible("1.0").is_err());
    assert!(assert_wit_compatible("").is_err());
}
