//! Plugin manifest types. [ADR-014, FR-PLUG-1, M11]
//!
//! A manifest declares a plugin's identity, required capabilities,
//! contributed UI elements, and the WIT world it targets.

use serde::{Deserialize, Serialize};

/// Top-level plugin manifest, serialized as JSON.
///
/// Parsed and validated on discovery. A manifest declaring capabilities
/// the current app version's WIT world does not provide is rejected
/// with a clear version-mismatch reason. [SDS §11.1]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PluginManifest {
    /// Unique identifier (reverse-domain recommended, e.g. `com.example.word-counter`).
    pub id: String,
    /// Human-readable display name.
    pub name: String,
    /// Semver version string.
    pub version: String,
    /// Plugin author name or organization.
    pub author: String,
    /// Short description of what the plugin does.
    pub description: String,
    /// The WIT world this plugin targets (e.g. `pdf-platform:plugin@1`).
    pub wit_world: String,
    /// Capabilities required by this plugin. [FR-PLUG-2, ADR-014 §3]
    pub capabilities: Vec<Capability>,
    /// Panel contributions (declarative schemas rendered by the shell). [FR-PLUG-1]
    #[serde(default)]
    pub panels: Vec<PanelContribution>,
    /// Tool contributions (menu items / toolbar buttons). [FR-PLUG-1]
    #[serde(default)]
    pub tools: Vec<ToolContribution>,
    /// Job type registrations. [FR-PLUG-1]
    #[serde(default)]
    pub job_types: Vec<JobTypeContribution>,
}

/// A capability a plugin declares it needs. [ADR-014 §2, FR-PLUG-2]
///
/// Capabilities are explicit and least-privilege. Undeclared capabilities
/// are unlinkable, not merely denied — the host functions for ungranted
/// capabilities are absent from the WASM instance. [ADR-014 §2]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum Capability {
    /// Read text content from document pages. [FR-PLUG-1]
    ReadText,
    /// Read document structure (outline, layers, attachments). [FR-PLUG-1]
    ReadStructure,
    /// Read a specific raw object by number.
    ReadObject {
        /// 1-based PDF indirect object number.
        obj_num: u32,
    },
    /// Submit annotation Commands (undoable, attributable to plugin). [FR-PLUG-4]
    Annotate,
    /// Register custom job types with the JobScheduler. [FR-PLUG-1]
    RegisterJob,
    /// Contribute UI panels (declarative schemas). [FR-PLUG-1]
    ContributePanel,
    /// Brokered network access (requires explicit user consent). [ADR-016]
    Network,
    /// Brokered file read (scoped, requires consent). [ADR-016]
    ReadFile,
    /// Brokered file write (scoped, requires consent). [ADR-016]
    WriteFile,
}

impl Capability {
    /// Human-readable description for the grant UI.
    pub fn description(&self) -> &'static str {
        match self {
            Self::ReadText => "Read text content from document pages",
            Self::ReadStructure => "Read document structure (bookmarks, layers, attachments)",
            Self::ReadObject { .. } => "Read raw PDF object data",
            Self::Annotate => "Add annotations to the document (undoable)",
            Self::RegisterJob => "Register custom batch/job types",
            Self::ContributePanel => "Add panels to the application UI",
            Self::Network => "Access the network (requires user consent per call)",
            Self::ReadFile => "Read files from the filesystem (scoped, requires consent)",
            Self::WriteFile => "Write files to the filesystem (scoped, requires consent)",
        }
    }
}

/// A declarative panel contribution. [FR-PLUG-1, SDS §4.5]
///
/// Panel schemas are rendered by the shell; plugin code never executes
/// in the UI process. [ADR-014 §4]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PanelContribution {
    /// Unique panel identifier within this plugin.
    pub id: String,
    /// Human-readable label shown in the UI.
    pub label: String,
    /// Where the panel docks.
    #[serde(default)]
    pub position: PanelPosition,
}

/// Docking position for a plugin panel.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PanelPosition {
    /// Left sidebar (alongside outline/thumbnails).
    Left,
    /// Right sidebar.
    Right,
    /// Bottom panel (alongside diagnostics).
    Bottom,
}

impl Default for PanelPosition {
    fn default() -> Self {
        Self::Right
    }
}

/// A tool contribution (menu item or toolbar button). [FR-PLUG-1]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolContribution {
    /// Unique tool identifier within this plugin.
    pub id: String,
    /// Human-readable label.
    pub label: String,
    /// Menu path (e.g., "Plugins > Word Counter > Count").
    pub menu_path: String,
}

/// A job type registration. [FR-PLUG-1]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JobTypeContribution {
    /// Unique job type identifier.
    pub id: String,
    /// Human-readable label.
    pub label: String,
}

/// Errors during manifest parsing or validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestError {
    /// JSON parse failure.
    Parse(String),
    /// Missing required field.
    MissingField(&'static str),
    /// Invalid field value.
    InvalidField {
        /// Field name.
        field: &'static str,
        /// Reason.
        reason: String,
    },
    /// No capabilities declared (warn, not error — a plugin may be observation-only).
    NoCapabilities,
    /// The manifest targets a WIT world this host does not implement.
    ///
    /// The host offers exactly [`HOST_WIT_WORLD`]; a plugin built against a
    /// different one cannot be linked, so accepting it would defer a
    /// guaranteed failure to instantiation time.
    /// [FR-PLUG-5, DS-PLUG-VER-1, ADR-015, ADR-030, SDS §11.1]
    IncompatibleWitWorld {
        /// The world the manifest asked for.
        declared: String,
        /// The world this host implements.
        host: &'static str,
    },
}

impl std::fmt::Display for ManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(msg) => write!(f, "manifest parse error: {msg}"),
            Self::MissingField(field) => write!(f, "missing required field: {field}"),
            Self::InvalidField { field, reason } => {
                write!(f, "invalid field '{field}': {reason}")
            }
            Self::NoCapabilities => write!(f, "no capabilities declared"),
            Self::IncompatibleWitWorld { declared, host } => write!(
                f,
                "plugin targets WIT world '{declared}', but this host implements \
                 '{host}'. Rebuild the plugin against '{host}', or run a host \
                 version that provides '{declared}'."
            ),
        }
    }
}

impl std::error::Error for ManifestError {}

/// Parse and validate a plugin manifest from JSON bytes.
pub fn parse_manifest(data: &[u8]) -> Result<PluginManifest, ManifestError> {
    let manifest: PluginManifest =
        serde_json::from_slice(data).map_err(|e| ManifestError::Parse(e.to_string()))?;

    validate_manifest(&manifest)?;

    Ok(manifest)
}

/// The WIT world this host implements, and the only one it can link a plugin
/// against. Must track `plugin-sdk/wit/plugin.wit`'s `package` declaration.
/// [ADR-015, ADR-030]
pub const HOST_WIT_WORLD: &str = "pdf-platform:plugin@1";

/// Validate a parsed manifest.
fn validate_manifest(m: &PluginManifest) -> Result<(), ManifestError> {
    if m.id.is_empty() {
        return Err(ManifestError::MissingField("id"));
    }
    if m.name.is_empty() {
        return Err(ManifestError::MissingField("name"));
    }
    if m.version.is_empty() {
        return Err(ManifestError::MissingField("version"));
    }
    if m.wit_world.is_empty() {
        return Err(ManifestError::MissingField("wit_world"));
    }

    // Semver format check: at minimum X.Y.Z
    let parts: Vec<&str> = m.version.split('.').collect();
    if parts.len() < 3 || !parts.iter().all(|p| p.parse::<u32>().is_ok()) {
        return Err(ManifestError::InvalidField {
            field: "version",
            reason: format!("expected semver X.Y.Z, got '{}'", m.version),
        });
    }

    // A world this host does not implement cannot be linked, so accepting the
    // manifest would only defer a certain failure to instantiation time.
    // [FR-PLUG-5, DS-PLUG-VER-1, SDS §11.1]
    if m.wit_world != HOST_WIT_WORLD {
        return Err(ManifestError::IncompatibleWitWorld {
            declared: m.wit_world.clone(),
            host: HOST_WIT_WORLD,
        });
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_manifest() {
        let json = r#"{
            "id": "com.example.test",
            "name": "Test Plugin",
            "version": "1.0.0",
            "author": "Test Author",
            "description": "A test plugin",
            "wit_world": "pdf-platform:plugin@1",
            "capabilities": [{"type": "ReadText"}]
        }"#;
        let m = parse_manifest(json.as_bytes()).unwrap();
        assert_eq!(m.id, "com.example.test");
        assert_eq!(m.version, "1.0.0");
        assert_eq!(m.capabilities.len(), 1);
    }

    #[test]
    fn parse_full_manifest() {
        let json = r#"{
            "id": "com.example.full",
            "name": "Full Plugin",
            "version": "2.1.3",
            "author": "Author",
            "description": "Full",
            "wit_world": "pdf-platform:plugin@1",
            "capabilities": [
                {"type": "ReadText"},
                {"type": "Annotate"},
                {"type": "Network"}
            ],
            "panels": [
                {"id": "info", "label": "Info Panel", "position": "Right"}
            ],
            "tools": [
                {"id": "count", "label": "Count Words", "menu_path": "Plugins > Count"}
            ],
            "job_types": [
                {"id": "analyze", "label": "Analyze Document"}
            ]
        }"#;
        let m = parse_manifest(json.as_bytes()).unwrap();
        assert_eq!(m.capabilities.len(), 3);
        assert_eq!(m.panels.len(), 1);
        assert_eq!(m.tools.len(), 1);
        assert_eq!(m.job_types.len(), 1);
        assert_eq!(m.panels[0].position, PanelPosition::Right);
    }

    #[test]
    fn reject_empty_id() {
        let json = r#"{
            "id": "",
            "name": "Test",
            "version": "1.0.0",
            "author": "A",
            "description": "D",
            "wit_world": "w",
            "capabilities": []
        }"#;
        assert!(matches!(
            parse_manifest(json.as_bytes()),
            Err(ManifestError::MissingField("id"))
        ));
    }

    #[test]
    fn reject_bad_semver() {
        let json = r#"{
            "id": "x",
            "name": "X",
            "version": "1.0",
            "author": "A",
            "description": "D",
            "wit_world": "w",
            "capabilities": []
        }"#;
        assert!(matches!(
            parse_manifest(json.as_bytes()),
            Err(ManifestError::InvalidField { field: "version", .. })
        ));
    }

    #[test]
    fn reject_invalid_json() {
        assert!(matches!(
            parse_manifest(b"not json"),
            Err(ManifestError::Parse(_))
        ));
    }

    #[test]
    fn capability_description_coverage() {
        // Ensure every variant has a non-empty description.
        let caps = [
            Capability::ReadText,
            Capability::ReadStructure,
            Capability::ReadObject { obj_num: 1 },
            Capability::Annotate,
            Capability::RegisterJob,
            Capability::ContributePanel,
            Capability::Network,
            Capability::ReadFile,
            Capability::WriteFile,
        ];
        for cap in &caps {
            assert!(!cap.description().is_empty(), "missing description for {cap:?}");
        }
    }

    #[test]
    fn manifest_roundtrip() {
        let json = r#"{
            "id": "com.example.rt",
            "name": "RT",
            "version": "1.2.3",
            "author": "A",
            "description": "D",
            "wit_world": "pdf-platform:plugin@1",
            "capabilities": [{"type": "ReadText"}, {"type": "Annotate"}]
        }"#;
        let m = parse_manifest(json.as_bytes()).unwrap();
        let serialized = serde_json::to_string(&m).unwrap();
        let m2: PluginManifest = serde_json::from_str(&serialized).unwrap();
        assert_eq!(m, m2);
    }

    fn conformant() -> PluginManifest {
        PluginManifest {
            id: "com.example.word-counter".into(),
            name: "Word Counter".into(),
            version: "1.0.0".into(),
            author: "Example".into(),
            description: "Counts words".into(),
            wit_world: HOST_WIT_WORLD.into(),
            capabilities: Vec::new(),
            panels: Vec::new(),
            tools: Vec::new(),
            job_types: Vec::new(),
        }
    }

    #[test]
    fn accepts_a_manifest_targeting_the_host_world() {
        assert!(validate_manifest(&conformant()).is_ok());
    }

    #[test]
    fn rejects_a_manifest_targeting_a_different_wit_world() {
        // The PluginManifest doc comment and SDS §11.1 both say a manifest the
        // host's WIT world cannot serve is *rejected with a clear
        // version-mismatch reason*. Only non-emptiness was checked, so a
        // plugin built against a future world validated clean.
        // [FR-PLUG-5, DS-PLUG-VER-1, ADR-015, ADR-030]
        let mut manifest = conformant();
        manifest.wit_world = "pdf-platform:plugin@2".into();
        let error = validate_manifest(&manifest)
            .expect_err("an incompatible world must not validate");
        assert!(
            matches!(error, ManifestError::IncompatibleWitWorld { .. }),
            "{error:?}"
        );
        let shown = error.to_string();
        assert!(
            shown.contains("pdf-platform:plugin@2") && shown.contains(HOST_WIT_WORLD),
            "the reason must name both the declared and the supported world: {shown}"
        );
    }

    #[test]
    fn parse_manifest_enforces_the_wit_world_not_just_the_shape() {
        // cmd_plugin_validate bypassed parse_manifest and called serde
        // directly, so nothing here ran at all. This pins the entry point.
        let json = br#"{
            "id": "com.example.p",
            "name": "P",
            "version": "1.0.0",
            "author": "A",
            "description": "D",
            "wit_world": "pdf-platform:plugin@99",
            "capabilities": []
        }"#;
        assert!(matches!(
            parse_manifest(json),
            Err(ManifestError::IncompatibleWitWorld { .. })
        ));
    }
}
