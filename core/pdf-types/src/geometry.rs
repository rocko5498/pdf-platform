//! Geometric primitives. [SDS §3, ADR-025]
// ponytail: stub — define Rect/Point/Size at M0 implementation

/// Axis-aligned rectangle in PDF user-space units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    /// Left edge x coordinate.
    pub x: f32,
    /// Top edge y coordinate.
    pub y: f32,
    /// Width in user-space units.
    pub width: f32,
    /// Height in user-space units.
    pub height: f32,
}

/// Point in PDF user-space units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    /// x coordinate.
    pub x: f32,
    /// y coordinate.
    pub y: f32,
}
