//! Stub rasterize engine producing colored test patterns. [ADR-005, M0]
//!
//! Proves the render pipeline works end-to-end without PDFium.
//! Each page gets a distinct hue; output is a checkerboard pattern.
//! Swap this for `engine-pdfium` once the prebuilt is pinned.

use engine_api::rasterize::{Rasterize, RasterizeError, RasterizeRequest, TileOutput};

/// Stub engine that produces colored checkerboard test patterns.
pub struct StubEngine {
    page_count: u32,
}

impl StubEngine {
    /// Create a stub engine for a document with the given page count.
    pub fn new(page_count: u32) -> Self {
        Self { page_count }
    }
}

impl Rasterize for StubEngine {
    fn rasterize(&self, req: &RasterizeRequest) -> Result<TileOutput, RasterizeError> {
        if req.page_index >= self.page_count {
            return Err(RasterizeError::PageOutOfRange {
                requested: req.page_index,
                page_count: self.page_count,
            });
        }

        let w = req.rect.w;
        let h = req.rect.h;
        let mut pixels = vec![0u8; (w * h * 4) as usize];

        // Each page gets a distinct hue via a simple rotation.
        // page 0 = red, 1 = green, 2 = blue, 3 = yellow, etc.
        let hue_step = 60; // degrees
        let hue = (req.page_index as f32 * hue_step as f32) % 360.0;
        let (r, g, b) = hsv_to_rgb(hue, 0.7, 0.9);

        // Checkerboard: 32px squares, alternating full color and 60% brightness.
        let square = 32u32;
        for row in 0..h {
            for col in 0..w {
                let cx = (req.rect.x + col) / square;
                let cy = (req.rect.y + row) / square;
                let bright = (cx + cy) % 2 == 0;
                let factor = if bright { 1.0 } else { 0.6 };
                let idx = ((row * w + col) * 4) as usize;
                pixels[idx] = (r as f32 * factor) as u8;
                pixels[idx + 1] = (g as f32 * factor) as u8;
                pixels[idx + 2] = (b as f32 * factor) as u8;
                pixels[idx + 3] = 255;
            }
        }

        Ok(TileOutput {
            rgba_pixels: pixels,
            width: w,
            height: h,
        })
    }

    fn page_count(&self) -> u32 {
        self.page_count
    }
}

/// Convert HSV (h: 0-360, s: 0-1, v: 0-1) to RGB (0-255 each).
fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r1, g1, b1) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    (
        ((r1 + m) * 255.0) as u8,
        ((g1 + m) * 255.0) as u8,
        ((b1 + m) * 255.0) as u8,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_api::rasterize::TileRect;

    #[test]
    fn stub_produces_nonzero_pixels() {
        let engine = StubEngine::new(3);
        let out = engine
            .rasterize(&RasterizeRequest {
                page_index: 0,
                rect: TileRect { x: 0, y: 0, w: 256, h: 256 },
                scale: 1.0,
            })
            .unwrap();
        assert_eq!(out.width, 256);
        assert_eq!(out.height, 256);
        assert_eq!(out.rgba_pixels.len(), 256 * 256 * 4);
        // First pixel should be non-zero (colored)
        assert_ne!(out.rgba_pixels[0], 0);
        assert_eq!(out.rgba_pixels[3], 255); // alpha
    }

    #[test]
    fn stub_page_out_of_range() {
        let engine = StubEngine::new(2);
        let err = engine
            .rasterize(&RasterizeRequest {
                page_index: 5,
                rect: TileRect { x: 0, y: 0, w: 64, h: 64 },
                scale: 1.0,
            })
            .unwrap_err();
        match err {
            RasterizeError::PageOutOfRange { requested, page_count } => {
                assert_eq!(requested, 5);
                assert_eq!(page_count, 2);
            }
            _ => panic!("expected PageOutOfRange"),
        }
    }

    #[test]
    fn stub_checkerboard_pattern() {
        let engine = StubEngine::new(1);
        let out = engine
            .rasterize(&RasterizeRequest {
                page_index: 0,
                rect: TileRect { x: 0, y: 0, w: 64, h: 64 },
                scale: 1.0,
            })
            .unwrap();
        // Pixel at (0,0) should be bright, pixel at (32,0) should be dimmer
        let bright = out.rgba_pixels[0];
        let dim = out.rgba_pixels[(32 * 4) as usize];
        assert!(bright > dim, "bright={bright} should be > dim={dim}");
    }
}
