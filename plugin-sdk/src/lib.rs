//! Rust guest SDK for PDF Platform plugins. [ADR-014, ADR-015, M11]
//!
//! This crate provides the types and host-function declarations that a
//! plugin author imports to build a WASM plugin against the `pdf-platform:plugin@1`
//! WIT world.
//!
//! # Architecture
//!
//! - **Types** (always available): data structures matching the WIT interfaces.
//! - **Host functions** (feature `host`): FFI declarations linked by the
//!   Wasmtime runtime. Only available when building for the WASM target.
//! - **Plugin trait**: the interface plugin authors implement.
//!
//! # Quick start
//!
//! ```ignore
//! use pdf_platform_plugin_sdk::*;
//!
//! struct MyPlugin;
//!
//! impl Plugin for MyPlugin {
//!     fn init(&mut self, info: InitInfo) {
//!         // Store info, prepare state.
//!     }
//!     fn run(&mut self) {
//!         let page_count = host::get_page_count();
//!         host::log(LogLevel::Info, &format!("Document has {page_count} pages"));
//!     }
//!     fn shutdown(&mut self) {
//!         // Release resources.
//!     }
//! }
//! ```

#![warn(missing_docs)]

/// The current WIT world version this SDK supports.
///
/// Must match the version in `plugin-sdk/wit/plugin.wit`.
/// When the WIT world is updated, this version is bumped per semver.
pub const CURRENT_WIT_WORLD_VERSION: &str = "1.0.0";

/// The minimum WIT world version this SDK is backward-compatible with.
///
/// Per ADR-030, deprecated interfaces ship alongside successors for
/// >= 2 release trains.
pub const MINIMUM_WIT_WORLD_VERSION: &str = "1.0.0";

// ---------------------------------------------------------------------------
// Types matching the WIT world definitions
// ---------------------------------------------------------------------------

/// Information passed to the plugin during initialization.
#[derive(Debug, Clone)]
pub struct InitInfo {
    /// Capabilities granted to this plugin.
    pub capabilities: Vec<String>,
    /// The host API version.
    pub host_version: String,
    /// The plugin's own version.
    pub plugin_version: String,
}

/// A text span within a line.
#[derive(Debug, Clone)]
pub struct TextSpan {
    /// The text content.
    pub text: String,
    /// X position in PDF user-space.
    pub x: f32,
    /// Y position in PDF user-space.
    pub y: f32,
    /// Width in PDF user-space.
    pub width: f32,
    /// Height in PDF user-space.
    pub height: f32,
    /// Line index.
    pub line_index: u32,
    /// Word index within the line.
    pub word_index: u32,
    /// Whether from tagged structure.
    pub is_structured: bool,
}

/// A line of extracted text with geometry.
#[derive(Debug, Clone)]
pub struct TextLine {
    /// Line index.
    pub index: u32,
    /// Full text.
    pub text: String,
    /// X position.
    pub x: f32,
    /// Y position.
    pub y: f32,
    /// Width.
    pub width: f32,
    /// Height.
    pub height: f32,
}

/// The canonical text model for a page.
#[derive(Debug, Clone)]
pub struct PageText {
    /// 0-based page index.
    pub page_index: u32,
    /// Text lines.
    pub lines: Vec<TextLine>,
    /// Whether extraction is reliable.
    pub reliable: bool,
    /// Total character count.
    pub char_count: u32,
    /// Whether page has tagged structure.
    pub has_structure: bool,
}

/// An outline (bookmark) entry.
#[derive(Debug, Clone)]
pub struct OutlineEntry {
    /// Display title.
    pub title: String,
    /// Target page index.
    pub page: u32,
    /// Y offset.
    pub y: f32,
    /// Zoom level.
    pub zoom: f32,
    /// Nested children.
    pub children: Vec<OutlineEntry>,
}

/// An optional content group (layer).
#[derive(Debug, Clone)]
pub struct Layer {
    /// Layer identifier.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Whether visible.
    pub visible: bool,
    /// Whether locked.
    pub locked: bool,
    /// Whether on by default.
    pub default_on: bool,
    /// Nested children.
    pub children: Vec<Layer>,
}

/// An embedded file attachment.
#[derive(Debug, Clone)]
pub struct Attachment {
    /// File name.
    pub name: String,
    /// MIME type.
    pub mime_type: String,
    /// Size in bytes.
    pub size: u64,
    /// Description.
    pub description: String,
}

/// Specification for creating an annotation.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "host", derive(serde::Serialize))]
pub struct AnnotationSpec {
    /// Annotation type (e.g., "highlight", "sticky_note").
    pub annotation_type: String,
    /// 0-based page index.
    pub page_index: u32,
    /// Rect as "x,y,width,height".
    pub rect: String,
    /// Optional content text.
    pub contents: Option<String>,
    /// Optional color as "r,g,b,a".
    pub color: Option<String>,
}

/// Log levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum LogLevel {
    /// Trace level.
    Trace = 0,
    /// Debug level.
    Debug = 1,
    /// Info level.
    Info = 2,
    /// Warn level.
    Warn = 3,
    /// Error level.
    Error = 4,
}

// ---------------------------------------------------------------------------
// Plugin trait
// ---------------------------------------------------------------------------

/// The trait a plugin author implements. [ADR-014, FR-PLUG-1]
///
/// The host calls `init` first, then `run`, then `shutdown` on unload.
pub trait Plugin {
    /// Initialize the plugin. Called once before any other method.
    fn init(&mut self, info: InitInfo);

    /// Run the plugin's main logic. Called after init.
    fn run(&mut self);

    /// Shut down the plugin. Called when unloading.
    fn shutdown(&mut self);
}

// ---------------------------------------------------------------------------
// Host function module (feature-gated)
// ---------------------------------------------------------------------------

/// Host function declarations and safe wrappers.
///
/// Only available when the `host` feature is enabled (WASM build target).
/// These functions are linked by the Wasmtime runtime at plugin instantiation.
#[cfg(feature = "host")]
pub mod host {
    use super::{AnnotationSpec, Attachment, Layer, LogLevel, OutlineEntry, PageText};

    extern "C" {
        fn host_log_raw(level: u32, ptr: *const u8, len: usize);
        fn host_get_page_count() -> u32;
        fn host_get_page_text(page_index: u32) -> i32;
        fn host_get_outline() -> i32;
        fn host_get_layers() -> i32;
        fn host_get_attachments() -> i32;
        fn host_submit_annotation(ptr: *const u8, len: usize) -> i64;
        fn host_free_handle(handle: i32);
    }

    /// Write a log message to the host diagnostics system. [ADR-020, FR-DIAG-1]
    pub fn log(level: LogLevel, message: &str) {
        let bytes = message.as_bytes();
        unsafe {
            host_log_raw(level as u32, bytes.as_ptr(), bytes.len());
        }
    }

    /// Get the total number of pages in the document.
    pub fn get_page_count() -> u32 {
        unsafe { host_get_page_count() }
    }

    /// Get the canonical text model for a page.
    pub fn get_page_text(_page_index: u32) -> Option<PageText> {
        // TODO: implement via shared-memory protocol with the host.
        None
    }

    /// Get the document outline (bookmarks).
    pub fn get_outline() -> Option<Vec<OutlineEntry>> {
        let handle = unsafe { host_get_outline() };
        if handle < 0 {
            return None;
        }
        unsafe { host_free_handle(handle) };
        Some(Vec::new())
    }

    /// Get optional content groups (layers).
    pub fn get_layers() -> Option<Vec<Layer>> {
        let handle = unsafe { host_get_layers() };
        if handle < 0 {
            return None;
        }
        unsafe { host_free_handle(handle) };
        Some(Vec::new())
    }

    /// Get embedded file attachments.
    pub fn get_attachments() -> Option<Vec<Attachment>> {
        let handle = unsafe { host_get_attachments() };
        if handle < 0 {
            return None;
        }
        unsafe { host_free_handle(handle) };
        Some(Vec::new())
    }

    /// Submit an annotation to the document.
    ///
    /// Returns the annotation ID on success. The annotation is undoable
    /// and attributed to this plugin. [FR-PLUG-4, ADR-013]
    pub fn submit_annotation(spec: &AnnotationSpec) -> Result<u64, &'static str> {
        let json = serde_json::to_string(spec)
            .map_err(|_| "failed to serialize annotation spec")?;
        let bytes = json.as_bytes();
        let result = unsafe { host_submit_annotation(bytes.as_ptr(), bytes.len()) };
        if result < 0 {
            Err("annotation submission failed")
        } else {
            Ok(result as u64)
        }
    }
}

// ---------------------------------------------------------------------------
// Tests (types only — no FFI)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_level_repr() {
        assert_eq!(LogLevel::Trace as u8, 0);
        assert_eq!(LogLevel::Debug as u8, 1);
        assert_eq!(LogLevel::Info as u8, 2);
        assert_eq!(LogLevel::Warn as u8, 3);
        assert_eq!(LogLevel::Error as u8, 4);
    }

    #[test]
    fn annotation_spec_construction() {
        let spec = AnnotationSpec {
            annotation_type: "highlight".into(),
            page_index: 0,
            rect: "100,200,50,12".into(),
            contents: None,
            color: Some("1.0,1.0,0.0,0.5".into()),
        };
        assert_eq!(spec.annotation_type, "highlight");
        assert_eq!(spec.page_index, 0);
        assert!(spec.contents.is_none());
        assert!(spec.color.is_some());
    }

    #[test]
    fn init_info_construction() {
        let info = InitInfo {
            capabilities: vec!["ReadText".into(), "Annotate".into()],
            host_version: "1.0.0".into(),
            plugin_version: "2.0.0".into(),
        };
        assert_eq!(info.capabilities.len(), 2);
        assert_eq!(info.host_version, "1.0.0");
    }

    #[test]
    fn page_text_construction() {
        let pt = PageText {
            page_index: 0,
            lines: vec![TextLine {
                index: 0,
                text: "Hello, world!".into(),
                x: 72.0,
                y: 720.0,
                width: 200.0,
                height: 12.0,
            }],
            reliable: true,
            char_count: 13,
            has_structure: false,
        };
        assert_eq!(pt.lines.len(), 1);
        assert!(pt.reliable);
    }

    #[test]
    fn outline_entry_recursive() {
        let entry = OutlineEntry {
            title: "Chapter 1".into(),
            page: 0,
            y: 0.0,
            zoom: 1.0,
            children: vec![OutlineEntry {
                title: "Section 1.1".into(),
                page: 1,
                y: 100.0,
                zoom: 1.0,
                children: Vec::new(),
            }],
        };
        assert_eq!(entry.children.len(), 1);
        assert_eq!(entry.children[0].title, "Section 1.1");
    }

    #[test]
    fn layer_construction() {
        let layer = Layer {
            id: "layer-1".into(),
            name: "Background".into(),
            visible: true,
            locked: false,
            default_on: true,
            children: Vec::new(),
        };
        assert!(layer.visible);
        assert!(!layer.locked);
    }

    #[test]
    fn attachment_construction() {
        let att = Attachment {
            name: "data.csv".into(),
            mime_type: "text/csv".into(),
            size: 1024,
            description: "Embedded data".into(),
        };
        assert_eq!(att.size, 1024);
    }
}
