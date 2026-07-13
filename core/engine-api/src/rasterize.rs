//! Rasterization engine trait. [ADR-005, ADR-007, GR-4]
//!
//! All engine backends implement this trait. Application code calls only through it.
//! No direct PDFium/MuPDF calls outside the engine crate. [GR-4]

use std::fmt;

/// Device-space rectangle within a page (pixels, origin top-left).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileRect {
    /// Left edge in device pixels.
    pub x: u32,
    /// Top edge in device pixels.
    pub y: u32,
    /// Width in device pixels.
    pub w: u32,
    /// Height in device pixels.
    pub h: u32,
}

/// Request to rasterize a region of a page.
#[derive(Debug, Clone)]
pub struct RasterizeRequest {
    /// 0-based page index.
    pub page_index: u32,
    /// Region to render within the page's device space.
    pub rect: TileRect,
    /// Scale factor (1.0 = 72 DPI device space).
    pub scale: f32,
}

/// Rasterized pixel output.
#[derive(Debug, Clone)]
pub struct TileOutput {
    /// RGBA8 pixel data, tightly packed, row-major.
    pub rgba_pixels: Vec<u8>,
    /// Output width in pixels.
    pub width: u32,
    /// Output height in pixels.
    pub height: u32,
}

/// Errors from rasterization.
#[derive(Debug)]
pub enum RasterizeError {
    /// Requested page index exceeds document page count.
    PageOutOfRange {
        /// Requested page index.
        requested: u32,
        /// Actual page count.
        page_count: u32,
    },
    /// Backend-specific error.
    Engine(String),
}

impl fmt::Display for RasterizeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PageOutOfRange { requested, page_count } => {
                write!(f, "page {requested} out of range (document has {page_count} pages)")
            }
            Self::Engine(msg) => write!(f, "engine error: {msg}"),
        }
    }
}

impl std::error::Error for RasterizeError {}

/// Rasterization capability. All engine backends implement this. [ADR-005, GR-4]
///
/// Application code MUST call through this trait; no direct PDFium calls. [GR-4]
pub trait Rasterize: Send + Sync {
    /// Render a page region into RGBA8 pixels.
    fn rasterize(&self, req: &RasterizeRequest) -> Result<TileOutput, RasterizeError>;

    /// Total number of pages in the document.
    fn page_count(&self) -> u32;
}
