//! Geometric primitives. [SDS §3, ADR-025]
// ponytail: stub — define Rect/Point/Size at M0 implementation

/// Axis-aligned rectangle in PDF user-space units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Point in PDF user-space units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}
