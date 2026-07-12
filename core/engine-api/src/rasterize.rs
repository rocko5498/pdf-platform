//! Rasterization engine trait. [ADR-005, ADR-007, GR-4]
// ponytail: stub — define RasterizeRequest/TileOutput at M0 implementation

/// All rasterize calls must go through this trait; no direct PDFium calls. [GR-4]
pub trait Rasterize: Send + Sync {
    // ponytail: stub
}
