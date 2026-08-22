//! Appearance comparison between two rendered pages. [FR-CMP-1, FR-CMP-2]
//!
//! FR-CMP-1 requires the platform to "identify differences in text content
//! **and in visual/page appearance**". Text comparison lives in
//! `text_extract::compare`; nothing compared appearance, so a change that
//! leaves the text identical — a moved figure, a different logo, a redaction
//! box drawn over content, a font substitution — was invisible to `compare`.
//!
//! What this does: renders both pages through the worker and compares them
//! pixel by pixel, reporting the fraction that differs beyond a per-channel
//! tolerance. What it deliberately does not do is claim perceptual judgement —
//! there is no SSIM, no structural similarity, no "looks the same to a human"
//! model here. A reader is told the proportion of pixels that differ and the
//! largest channel delta, which is a measurement, not an opinion. [PRIN-6]

use protocol::handles::PixelFormat;

/// Per-channel difference ignored as rendering noise.
///
/// Anti-aliasing and subpixel positioning make two renders of the *same*
/// content differ by a few levels on glyph edges. Zero tolerance would report
/// every page as changed, which is worse than useless; this is deliberately
/// small so a real appearance change is never hidden by it.
pub const DEFAULT_CHANNEL_TOLERANCE: u8 = 8;

/// Why two pages could not be compared as images.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VisualCompareError {
    /// The rasters describe different pixel counts than their dimensions imply.
    MalformedRaster {
        /// Which side was malformed: 0 = earlier document, 1 = later.
        side: u8,
    },
    /// One side rendered nothing.
    EmptyRaster {
        /// Which side was empty: 0 = earlier document, 1 = later.
        side: u8,
    },
}

impl std::fmt::Display for VisualCompareError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MalformedRaster { side } => {
                write!(f, "raster {side} has a pixel count its dimensions do not explain")
            }
            Self::EmptyRaster { side } => write!(f, "raster {side} is empty"),
        }
    }
}

impl std::error::Error for VisualCompareError {}

/// The measured appearance difference between two renders of one page.
#[derive(Debug, Clone, PartialEq)]
pub struct VisualPageDiff {
    /// Zero-based page index.
    pub page_index: u32,
    /// Pixels whose colour differs beyond the tolerance.
    pub changed_pixels: u64,
    /// Pixels compared. When the two pages differ in size this is the larger
    /// area, so the fraction cannot be flattered by comparing a small overlap.
    pub total_pixels: u64,
    /// Largest per-channel difference seen.
    pub max_channel_delta: u8,
    /// Set when the pages are not the same size — a geometry change is itself
    /// an appearance difference, and comparing only the overlap would hide it.
    pub geometry_differs: bool,
}

impl VisualPageDiff {
    /// Proportion of compared pixels that differ, in `0.0..=1.0`.
    #[must_use]
    pub fn changed_fraction(&self) -> f32 {
        if self.total_pixels == 0 {
            return 0.0;
        }
        self.changed_pixels as f32 / self.total_pixels as f32
    }

    /// Whether any difference was measured at all.
    #[must_use]
    pub fn is_identical(&self) -> bool {
        self.changed_pixels == 0 && !self.geometry_differs
    }
}

/// One rendered page: RGBA8 pixels and their dimensions.
#[derive(Debug, Clone)]
pub struct RenderedPage {
    /// Zero-based page index.
    pub page_index: u32,
    /// Raster width in pixels.
    pub width: u32,
    /// Raster height in pixels.
    pub height: u32,
    /// RGBA8 pixel data, four bytes per pixel.
    pub pixels: Vec<u8>,
}

impl RenderedPage {
    /// Pixel format this comparison assumes. [SDS §6.4]
    #[must_use]
    pub const fn format() -> PixelFormat {
        PixelFormat::Rgba8
    }

    fn expected_len(&self) -> usize {
        (self.width as usize) * (self.height as usize) * 4
    }
}

/// Compare two renders of the same page.
///
/// `tolerance` is the per-channel difference treated as rendering noise; pass
/// [`DEFAULT_CHANNEL_TOLERANCE`] unless you have a reason not to.
///
/// Pages of different sizes are reported as `geometry_differs`, and every pixel
/// outside the shared area counts as changed: a page that grew has changed in
/// appearance, and measuring only the overlap would report a smaller difference
/// the more the page changed. [FR-CMP-1, PRIN-6]
pub fn compare_pages(
    before: &RenderedPage,
    after: &RenderedPage,
    tolerance: u8,
) -> Result<VisualPageDiff, VisualCompareError> {
    if before.pixels.is_empty() {
        return Err(VisualCompareError::EmptyRaster { side: 0 });
    }
    if after.pixels.is_empty() {
        return Err(VisualCompareError::EmptyRaster { side: 1 });
    }
    if before.pixels.len() != before.expected_len() {
        return Err(VisualCompareError::MalformedRaster { side: 0 });
    }
    if after.pixels.len() != after.expected_len() {
        return Err(VisualCompareError::MalformedRaster { side: 1 });
    }

    let geometry_differs = before.width != after.width || before.height != after.height;
    let shared_width = before.width.min(after.width);
    let shared_height = before.height.min(after.height);

    let larger_area =
        u64::from(before.width.max(after.width)) * u64::from(before.height.max(after.height));
    let shared_area = u64::from(shared_width) * u64::from(shared_height);

    let mut changed_pixels = larger_area - shared_area; // outside the overlap
    let mut max_channel_delta = 0u8;

    for y in 0..shared_height {
        for x in 0..shared_width {
            let a = ((y * before.width + x) * 4) as usize;
            let b = ((y * after.width + x) * 4) as usize;
            let mut differs = false;
            for channel in 0..4 {
                let delta = before.pixels[a + channel].abs_diff(after.pixels[b + channel]);
                max_channel_delta = max_channel_delta.max(delta);
                if delta > tolerance {
                    differs = true;
                }
            }
            if differs {
                changed_pixels += 1;
            }
        }
    }

    Ok(VisualPageDiff {
        page_index: before.page_index,
        changed_pixels,
        total_pixels: larger_area,
        max_channel_delta,
        geometry_differs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(width: u32, height: u32, fill: u8) -> RenderedPage {
        RenderedPage {
            page_index: 0,
            width,
            height,
            pixels: vec![fill; (width * height * 4) as usize],
        }
    }

    #[test]
    fn identical_renders_report_no_difference() {
        let diff = compare_pages(&page(4, 4, 255), &page(4, 4, 255), DEFAULT_CHANNEL_TOLERANCE)
            .expect("compare");
        assert!(diff.is_identical());
        assert_eq!(diff.changed_fraction(), 0.0);
        assert_eq!(diff.total_pixels, 16);
    }

    #[test]
    fn a_changed_region_is_measured_not_judged() {
        let mut after = page(4, 4, 255);
        // Blacken one pixel.
        for channel in 0..4 {
            after.pixels[channel] = 0;
        }
        let diff = compare_pages(&page(4, 4, 255), &after, DEFAULT_CHANNEL_TOLERANCE)
            .expect("compare");
        assert_eq!(diff.changed_pixels, 1);
        assert_eq!(diff.max_channel_delta, 255);
        assert!((diff.changed_fraction() - 1.0 / 16.0).abs() < f32::EPSILON);
    }

    #[test]
    fn antialiasing_noise_is_below_the_tolerance() {
        // Two renders of the same content differ by a few levels on glyph
        // edges. Reporting that as an appearance change would make the feature
        // useless, so the tolerance exists — and is small enough that a real
        // change cannot hide under it.
        let mut after = page(4, 4, 255);
        for pixel in after.pixels.iter_mut() {
            *pixel = 255 - DEFAULT_CHANNEL_TOLERANCE;
        }
        let diff = compare_pages(&page(4, 4, 255), &after, DEFAULT_CHANNEL_TOLERANCE)
            .expect("compare");
        assert!(diff.is_identical(), "{diff:?}");
        assert_eq!(diff.max_channel_delta, DEFAULT_CHANNEL_TOLERANCE);
    }

    #[test]
    fn a_page_that_changed_size_is_not_flattered_by_its_overlap() {
        // Comparing only the shared area would report "0% changed" for a page
        // that doubled in height with identical content on top.
        let diff = compare_pages(&page(4, 4, 255), &page(4, 8, 255), DEFAULT_CHANNEL_TOLERANCE)
            .expect("compare");
        assert!(diff.geometry_differs);
        assert!(!diff.is_identical());
        assert_eq!(diff.total_pixels, 32);
        assert_eq!(diff.changed_pixels, 16, "the area with no counterpart counts as changed");
    }

    #[test]
    fn a_raster_that_lies_about_its_size_is_rejected() {
        let mut broken = page(4, 4, 255);
        broken.pixels.truncate(10);
        let error = compare_pages(&page(4, 4, 255), &broken, DEFAULT_CHANNEL_TOLERANCE)
            .expect_err("must reject");
        assert_eq!(error, VisualCompareError::MalformedRaster { side: 1 });
    }

    #[test]
    fn an_empty_raster_is_an_error_not_a_clean_bill_of_health() {
        // A page that failed to render must never read as "no visual change".
        let empty = RenderedPage { page_index: 0, width: 0, height: 0, pixels: Vec::new() };
        let error = compare_pages(&empty, &page(4, 4, 255), DEFAULT_CHANNEL_TOLERANCE)
            .expect_err("must reject");
        assert_eq!(error, VisualCompareError::EmptyRaster { side: 0 });
    }
}
