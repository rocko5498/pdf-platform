//! Shared primitives used across all layers. No upstream deps. [ADR-025 foundation]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod geometry; // page-space / device-space rects, points, scale
