//! Document-structure engine trait and data models. [ADR-005, GR-4]
//!
//! Defines the types for outline/bookmarks, layers, attachments, and page metadata
//! that navigation panels consume. Backends implement the `Structure` trait.

use std::fmt;

// ---------------------------------------------------------------------------
// Outline / Bookmark
// ---------------------------------------------------------------------------

/// A single entry in the document outline (bookmark tree). [FR-BOOK]
#[derive(Debug, Clone)]
pub struct OutlineEntry {
    /// Display title of this entry.
    pub title: String,
    /// Destination page index (0-based).
    pub page: u32,
    /// Vertical scroll offset in PDF points (destination Y).
    pub y: f32,
    /// Zoom level at the destination (0.0 = inherit current).
    pub zoom: f32,
    /// Nested children (sub-bookmarks).
    pub children: Vec<OutlineEntry>,
}

/// The document's outline (table of contents). May be empty.
#[derive(Debug, Clone, Default)]
pub struct Outline {
    /// Top-level entries in document order.
    pub entries: Vec<OutlineEntry>,
}

impl Outline {
    /// Total number of entries (recursive count).
    pub fn total_count(&self) -> usize {
        fn count(entries: &[OutlineEntry]) -> usize {
            entries.iter().map(|e| 1 + count(&e.children)).sum()
        }
        count(&self.entries)
    }

    /// Whether the outline is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Layers (Optional Content Groups)
// ---------------------------------------------------------------------------

/// An optional content group (layer) with its current visibility state. [FR-LAYER]
#[derive(Debug, Clone)]
pub struct Layer {
    /// Unique identifier for this layer.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Whether this layer is currently visible.
    pub visible: bool,
    /// Whether this layer is locked (user cannot toggle).
    pub locked: bool,
    /// Whether this layer is the default visible state.
    pub default_on: bool,
    /// Nested child layers (parent groups).
    pub children: Vec<Layer>,
}

/// The document's optional content groups.
#[derive(Debug, Clone, Default)]
pub struct Layers {
    /// Top-level layer groups.
    pub groups: Vec<Layer>,
}

impl Layers {
    /// Whether any layers exist.
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    /// Total number of layers (recursive count).
    pub fn total_count(&self) -> usize {
        fn count(groups: &[Layer]) -> usize {
            groups.iter().map(|g| 1 + count(&g.children)).sum()
        }
        count(&self.groups)
    }
}

// ---------------------------------------------------------------------------
// Attachments (Embedded Files)
// ---------------------------------------------------------------------------

/// Metadata for an embedded file attachment. [FR-EMB]
#[derive(Debug, Clone)]
pub struct Attachment {
    /// File name.
    pub name: String,
    /// MIME type (if known).
    pub mime_type: Option<String>,
    /// File size in bytes.
    pub size: u64,
    /// Creation date (PDF date string, if present).
    pub created: Option<String>,
    /// Modification date (PDF date string, if present).
    pub modified: Option<String>,
    /// Description (if present in the file specification).
    pub description: Option<String>,
}

/// The document's embedded file attachments.
#[derive(Debug, Clone, Default)]
pub struct Attachments {
    /// Embedded files in document order.
    pub files: Vec<Attachment>,
}

impl Attachments {
    /// Whether any attachments exist.
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Page metadata (for thumbnails)
// ---------------------------------------------------------------------------

/// Metadata for a single page, used by thumbnail generation and page panels.
#[derive(Debug, Clone)]
pub struct PageMeta {
    /// 0-based page index.
    pub index: u32,
    /// Page width in PDF points.
    pub width: f32,
    /// Page height in PDF points.
    pub height: f32,
    /// Page rotation in degrees (0, 90, 180, 270).
    pub rotation: u32,
    /// Optional page label (e.g., "iii", "12-A").
    pub label: Option<String>,
}

// ---------------------------------------------------------------------------
// Structure trait
// ---------------------------------------------------------------------------

/// Errors from structure queries.
#[derive(Debug)]
pub enum StructureError {
    /// No document is loaded.
    NoDocument,
    /// Requested data is not available (e.g., no outline in document).
    NotAvailable(String),
    /// Backend-specific error.
    Engine(String),
}

impl fmt::Display for StructureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoDocument => write!(f, "no document loaded"),
            Self::NotAvailable(msg) => write!(f, "not available: {msg}"),
            Self::Engine(msg) => write!(f, "engine error: {msg}"),
        }
    }
}

impl std::error::Error for StructureError {}

/// Document-structure capability. [ADR-005, GR-4]
///
/// All engine backends implement this trait to provide outline, layer,
/// attachment, and page metadata queries.
pub trait Structure: Send + Sync {
    /// Get the document outline (table of contents).
    fn outline(&self) -> Result<Outline, StructureError>;

    /// Get the optional content groups (layers).
    fn layers(&self) -> Result<Layers, StructureError>;

    /// Get embedded file attachments.
    fn attachments(&self) -> Result<Attachments, StructureError>;

    /// Get metadata for all pages (used by thumbnail panel).
    fn page_meta(&self) -> Result<Vec<PageMeta>, StructureError>;
}
