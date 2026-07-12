//! Resolution and scale types. [SDS §6, ADR-007]
// ponytail: stub — tile constants live here at M0 implementation

/// Device-pixel scale factor (physical px / logical dp).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScaleFactor(pub f32);

/// Fixed tile side length in logical device pixels. [ADR-007]
pub const TILE_SIZE_DP: u32 = 256;
