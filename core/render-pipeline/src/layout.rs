//! Grid layout system and viewport state management. [SDS §6, DS-SCROLL, DS-ZOOM]
//!
//! Translates the user's view state (scroll position, zoom level, layout mode)
//! into concrete page positions and viewport regions that the render scheduler
//! can decompose into tile requests.
//!
//! ## Design
//!
//! The coordinate system has two layers:
//! - **Document space**: pages are positioned in an infinite vertical scroll
//!   area. Each page has a fixed size in points (PDF user space). Gaps between
//!   pages and center gutters for facing layouts are added here.
//! - **Device space**: document-space coordinates are multiplied by the zoom
//!   scale to produce device pixels. Tile requests operate in device space.
//!
//! ## Layout modes
//!
//! Per `DS-SCROLL-1` and `SDS §6.1`:
//! - **Single**: one page fills the viewport; scroll advances by one page.
//! - **Continuous**: pages stack vertically with inter-page gaps; smooth scroll.
//! - **Facing**: two pages side by side (like an open book); page advance is 2.
//! - **ContinuousFacing**: facing pairs stacked vertically; smooth scroll.

use crate::scheduler::{Viewport, ViewportRegion};

// ---------------------------------------------------------------------------
// Page geometry
// ---------------------------------------------------------------------------

/// Dimensions of a single page in PDF user-space points (1/72 inch).
///
/// These are the intrinsic page dimensions from the page tree, before any
/// rotation or scaling.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageGeometry {
    /// Width in points (before rotation).
    pub width: f32,
    /// Height in points (before rotation).
    pub height: f32,
    /// Page rotation in degrees (0, 90, 180, 270) from `/Rotate`.
    pub rotation: u32,
}

impl PageGeometry {
    /// Effective width after applying rotation.
    pub fn effective_width(&self) -> f32 {
        match self.rotation % 360 {
            90 | 270 => self.height,
            _ => self.width,
        }
    }

    /// Effective height after applying rotation.
    pub fn effective_height(&self) -> f32 {
        match self.rotation % 360 {
            90 | 270 => self.width,
            _ => self.height,
        }
    }
}

// ---------------------------------------------------------------------------
// Page layout modes
// ---------------------------------------------------------------------------

/// How pages are arranged in the viewport. [DS-SCROLL-1, SDS §6.1]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageLayout {
    /// One page fills the viewport. Scroll advances by whole pages.
    Single,
    /// Pages stack vertically with gaps. Smooth pixel-level scroll.
    Continuous,
    /// Two pages side by side (book spread). Scroll advances by pairs.
    Facing,
    /// Facing pairs stacked vertically. Smooth scroll through spreads.
    ContinuousFacing,
}

impl PageLayout {
    /// Whether this layout mode supports smooth (sub-page) scrolling.
    pub fn is_continuous(&self) -> bool {
        matches!(self, Self::Continuous | Self::ContinuousFacing)
    }

    /// Whether this layout shows pages side by side.
    pub fn is_facing(&self) -> bool {
        matches!(self, Self::Facing | Self::ContinuousFacing)
    }

    /// Number of pages visible per "step" in non-continuous mode.
    pub fn pages_per_step(&self) -> u32 {
        if self.is_facing() { 2 } else { 1 }
    }
}

// ---------------------------------------------------------------------------
// Scale bucketing
// ---------------------------------------------------------------------------

/// Standard zoom levels for cache reuse. [SDS §6.2]
///
/// Tiles are rasterized at these fixed scale factors. During interactive zoom,
/// the GPU scales the nearest cached bucket (instant, slightly soft); when the
/// gesture settles, the scheduler requests crisp tiles at the nearest bucket.
const SCALE_BUCKETS: &[f32] = &[
    0.125, 0.25, 0.333, 0.5, 0.667, 0.75, 1.0, 1.25, 1.5, 2.0, 3.0, 4.0, 6.0, 8.0,
];

/// Quantize a scale factor to the nearest bucket for cache key stability.
///
/// Returns `(bucket_scale, fraction_into_bucket)` where `fraction_into_bucket`
/// is 0.0 when exactly on a bucket and 0.5 when midway between buckets.
pub fn bucket_scale(scale: f32) -> (f32, f32) {
    if SCALE_BUCKETS.is_empty() {
        return (scale, 0.0);
    }
    let mut best = SCALE_BUCKETS[0];
    let mut best_dist = (scale - best).abs();
    for &bucket in &SCALE_BUCKETS[1..] {
        let dist = (scale - bucket).abs();
        if dist < best_dist {
            best = bucket;
            best_dist = dist;
        }
    }
    // Compute how far we are between this bucket and the next/prev (0.0..1.0).
    let frac = if let Some(&next) = SCALE_BUCKETS.iter().find(|&&b| b > best) {
        let range = next - best;
        if range > 0.0 {
            (scale - best) / range
        } else {
            0.0
        }
    } else {
        0.0
    };
    (best, frac.clamp(0.0, 1.0))
}

/// The number of standard zoom buckets.
pub fn scale_bucket_count() -> usize {
    SCALE_BUCKETS.len()
}

/// Get a scale bucket by index.
pub fn scale_bucket(index: usize) -> Option<f32> {
    SCALE_BUCKETS.get(index).copied()
}

// ---------------------------------------------------------------------------
// Viewport state
// ---------------------------------------------------------------------------

/// The user's current view state, maintained by the canvas widget.
///
/// From this state + page geometries, the `PagePositioner` computes the
/// concrete `Viewport` for the render scheduler.
#[derive(Debug, Clone)]
pub struct ViewportState {
    /// Current scroll offset in document-space points (top of viewport).
    pub scroll_y: f32,
    /// Horizontal scroll offset in document-space points (for wide pages).
    pub scroll_x: f32,
    /// Zoom scale factor (1.0 = 100%, one PDF point = one device pixel).
    pub scale: f32,
    /// Minimum allowed scale (default 0.125 = 12.5%).
    pub min_scale: f32,
    /// Maximum allowed scale (default 8.0 = 800%).
    pub max_scale: f32,
    /// View rotation in degrees (0, 90, 180, 270).
    pub rotation: u32,
    /// Active page layout mode.
    pub layout: PageLayout,
    /// Viewport width in device pixels (the widget's visible area).
    pub viewport_width: f32,
    /// Viewport height in device pixels (the widget's visible area).
    pub viewport_height: f32,
    /// Inter-page gap in document-space points. [DS-CANVAS-6]
    pub page_gap: f32,
    /// Center gutter for facing layouts in document-space points.
    pub facing_gutter: f32,
    /// Scroll velocity in document-space points per second (positive = scrolling down).
    /// Updated by the canvas widget on each scroll event. [SDS §6.9]
    pub scroll_velocity_y: f32,
}

impl ViewportState {
    /// Create a default viewport state for a document.
    pub fn new(viewport_width: f32, viewport_height: f32) -> Self {
        Self {
            scroll_y: 0.0,
            scroll_x: 0.0,
            scale: 1.0,
            min_scale: 0.125,
            max_scale: 8.0,
            rotation: 0,
            layout: PageLayout::Continuous,
            viewport_width,
            viewport_height,
            page_gap: 20.0,
            facing_gutter: 8.0,
            scroll_velocity_y: 0.0,
        }
    }

    /// The bucketed scale for tile cache keys.
    pub fn bucketed_scale(&self) -> f32 {
        bucket_scale(self.scale).0
    }

    // -----------------------------------------------------------------------
    // Zoom operations
    // -----------------------------------------------------------------------

    /// Set the zoom scale, clamped to min/max bounds. [DS-ZOOM-4]
    pub fn set_scale(&mut self, scale: f32) {
        self.scale = scale.clamp(self.min_scale, self.max_scale);
    }

    /// Zoom by a multiplicative factor, anchoring toward a focal point.
    ///
    /// `factor` > 1.0 zooms in, < 1.0 zooms out. `focus_x`/`focus_y` are the
    /// device-pixel coordinates of the focal point (e.g. mouse pointer position
    /// within the canvas widget). The scroll position is adjusted so the
    /// document point under the focal point stays in place. [DS-ZOOM-2]
    pub fn zoom_by(&mut self, factor: f32, focus_x: f32, focus_y: f32) {
        let old_scale = self.scale;
        let new_scale = (old_scale * factor).clamp(self.min_scale, self.max_scale);
        if (new_scale - old_scale).abs() < f32::EPSILON {
            return;
        }

        // The document-space point under the focal point before zoom:
        // doc_x = scroll_x + focus_x / old_scale
        // doc_y = scroll_y + focus_y / old_scale
        //
        // After zoom, we want the same document point under the same focal point:
        // scroll_x_new = doc_x - focus_x / new_scale
        // scroll_y_new = doc_y - focus_y / new_scale
        let doc_x = self.scroll_x + focus_x / old_scale;
        let doc_y = self.scroll_y + focus_y / old_scale;
        self.scroll_x = doc_x - focus_x / new_scale;
        self.scroll_y = doc_y - focus_y / new_scale;
        self.scale = new_scale;
    }

    /// Zoom by a multiplicative factor, anchoring toward the viewport center.
    pub fn zoom_by_center(&mut self, factor: f32) {
        let cx = self.viewport_width / 2.0;
        let cy = self.viewport_height / 2.0;
        self.zoom_by(factor, cx, cy);
    }

    /// Zoom to an exact scale, anchoring toward the viewport center.
    pub fn zoom_to(&mut self, target_scale: f32) {
        let factor = target_scale / self.scale;
        self.zoom_by_center(factor);
    }

    /// Zoom to fit the entire page width in the viewport. [DS-ZOOM-1]
    ///
    /// Uses the first visible page's width to compute the scale.
    pub fn zoom_to_fit_width(&mut self, page_width: f32) {
        if page_width <= 0.0 {
            return;
        }
        let target = self.viewport_width / page_width;
        self.zoom_to(target);
    }

    /// Zoom to fit the entire page height in the viewport. [DS-ZOOM-1]
    pub fn zoom_to_fit_height(&mut self, page_height: f32) {
        if page_height <= 0.0 {
            return;
        }
        let target = self.viewport_height / page_height;
        self.zoom_to(target);
    }

    /// Zoom to fit the entire page in the viewport. [DS-ZOOM-1]
    pub fn zoom_to_fit_page(&mut self, page_width: f32, page_height: f32) {
        let scale_w = if page_width > 0.0 { self.viewport_width / page_width } else { f32::MAX };
        let scale_h = if page_height > 0.0 { self.viewport_height / page_height } else { f32::MAX };
        self.zoom_to(scale_w.min(scale_h));
    }

    /// Zoom to fit a rectangle (in document-space points) into the viewport.
    ///
    /// Used for marquee zoom: the user draws a rectangle and the view zooms
    /// to fill that rectangle. `rect_x`, `rect_y`, `rect_w`, `rect_h` are in
    /// document-space points. [DS-ZOOM-5]
    pub fn zoom_to_rect(&mut self, rect_x: f32, rect_y: f32, rect_w: f32, rect_h: f32) {
        if rect_w <= 0.0 || rect_h <= 0.0 {
            return;
        }
        let scale_w = self.viewport_width / rect_w;
        let scale_h = self.viewport_height / rect_h;
        let new_scale = scale_w.min(scale_h);

        // Center the rectangle in the viewport.
        self.scale = new_scale.clamp(self.min_scale, self.max_scale);
        self.scroll_x = rect_x - (self.viewport_width / self.scale - rect_w) / 2.0;
        self.scroll_y = rect_y - (self.viewport_height / self.scale - rect_h) / 2.0;
    }

    /// Step to the next higher standard zoom level. [DS-ZOOM-1]
    pub fn zoom_in_step(&mut self) {
        let current = self.scale;
        for &bucket in SCALE_BUCKETS {
            if bucket > current + f32::EPSILON {
                self.zoom_to(bucket);
                return;
            }
        }
    }

    /// Step to the next lower standard zoom level. [DS-ZOOM-1]
    pub fn zoom_out_step(&mut self) {
        let current = self.scale;
        for &bucket in SCALE_BUCKETS.iter().rev() {
            if bucket < current - f32::EPSILON {
                self.zoom_to(bucket);
                return;
            }
        }
    }

    /// Reset zoom to 100% (actual size), anchoring toward viewport center.
    pub fn zoom_to_actual_size(&mut self) {
        self.zoom_to(1.0);
    }
}

// ---------------------------------------------------------------------------
// Velocity-aware prefetch margin
// ---------------------------------------------------------------------------

/// Compute the prefetch margin based on scroll velocity. [SDS §6.9]
///
/// Fast scrolling widens the ring in the scroll direction to keep tiles warm
/// ahead of the user. The margin is clamped to `[min, max]` tile rows.
///
/// - At rest (velocity ≈ 0): margin = `min_margin` (default 2)
/// - At high velocity: margin scales up to `max_margin` (default 8)
///
/// The relationship is approximately linear, scaled so that a velocity of
/// one viewport-height-per-second reaches about 75% of the max margin.
pub fn compute_prefetch_margin(
    velocity_y: f32,
    viewport_height_tiles: f32,
    min_margin: u32,
    max_margin: u32,
) -> u32 {
    let velocity = velocity_y.abs();
    if velocity < 1.0 || viewport_height_tiles <= 0.0 {
        return min_margin;
    }
    // Normalize velocity to viewport-heights per second.
    let vh_per_sec = velocity / viewport_height_tiles;
    // Scale: 0..1 maps to min..max. At 1 viewport-height/sec, we're at ~75%.
    let t = (vh_per_sec / 1.33).min(1.0);
    let margin = min_margin as f32 + t * (max_margin - min_margin) as f32;
    margin.round() as u32
}

// ---------------------------------------------------------------------------
// Page positioner
// ---------------------------------------------------------------------------

/// Computes page positions in document-space coordinates from the viewport state.
///
/// This is the bridge between the user's scroll position and the concrete
/// page regions that the render scheduler needs.
pub struct PagePositioner {
    geometries: Vec<PageGeometry>,
}

impl PagePositioner {
    /// Create a positioner with the given page geometries.
    pub fn new(geometries: Vec<PageGeometry>) -> Self {
        Self { geometries }
    }

    /// Number of pages in the document.
    pub fn page_count(&self) -> u32 {
        self.geometries.len() as u32
    }

    /// Get the geometry for a page.
    pub fn geometry(&self, page: u32) -> Option<&PageGeometry> {
        self.geometries.get(page as usize)
    }

    /// Compute the Y offset (in document-space points) where a page starts.
    ///
    /// In continuous mode, pages stack vertically with gaps.
    /// In facing mode, pages are grouped into spreads.
    pub fn page_y_offset(&self, page: u32, layout: PageLayout, page_gap: f32, facing_gutter: f32) -> f32 {
        let _ = facing_gutter; // Used in Facing/ContinuousFacing branches only.
        match layout {
            PageLayout::Single => 0.0,
            PageLayout::Continuous => {
                let mut y = 0.0;
                for p in 0..page {
                    y += self.geometries[p as usize].effective_height();
                    y += page_gap;
                }
                y
            }
            PageLayout::Facing => {
                // In non-continuous facing, each "step" is a spread.
                // Page 0 is alone (recto), then pages 1-2, 3-4, etc.
                // For scroll positioning, we compute based on spread index.
                let spread_index = page / 2;
                let mut y = 0.0;
                for s in 0..spread_index {
                    let left = s * 2;
                    let right = s * 2 + 1;
                    let left_h = self.geometries.get(left as usize)
                        .map(|g| g.effective_height())
                        .unwrap_or(0.0);
                    let right_h = self.geometries.get(right as usize)
                        .map(|g| g.effective_height())
                        .unwrap_or(0.0);
                    y += left_h.max(right_h) + page_gap;
                }
                y
            }
            PageLayout::ContinuousFacing => {
                // Pages are grouped into spreads of 2. Both pages in a spread
                // share the same Y offset. We compute based on which spread
                // contains the target page.
                let spread_first = (page / 2) * 2;
                let mut y = 0.0;
                let mut s = 0u32;
                while s < spread_first {
                    let left_h = self.geometries.get(s as usize)
                        .map(|g| g.effective_height())
                        .unwrap_or(0.0);
                    let right_h = self.geometries.get((s + 1) as usize)
                        .map(|g| g.effective_height())
                        .unwrap_or(0.0);
                    y += left_h.max(right_h);
                    y += page_gap;
                    s += 2;
                }
                y
            }
        }
    }

    /// Compute the X offset for a page within its spread.
    ///
    /// In single/continuous modes, pages are centered horizontally.
    /// In facing modes, left pages are offset left and right pages offset right.
    pub fn page_x_offset(
        &self,
        page: u32,
        layout: PageLayout,
        container_width: f32,
        facing_gutter: f32,
    ) -> f32 {
        match layout {
            PageLayout::Single | PageLayout::Continuous => {
                // Center the page horizontally in the viewport.
                let page_w = self.geometries.get(page as usize)
                    .map(|g| g.effective_width())
                    .unwrap_or(0.0);
                (container_width - page_w).max(0.0) / 2.0
            }
            PageLayout::Facing | PageLayout::ContinuousFacing => {
                let is_right = page % 2 == 1;
                let pair_page = if is_right { page - 1 } else { page };

                let left_w = self.geometries.get(pair_page as usize)
                    .map(|g| g.effective_width())
                    .unwrap_or(0.0);
                let right_w = self.geometries.get((pair_page + 1) as usize)
                    .map(|g| g.effective_width())
                    .unwrap_or(0.0);

                let total_w = left_w + facing_gutter + right_w;
                let start_x = (container_width - total_w).max(0.0) / 2.0;

                if is_right {
                    start_x + left_w + facing_gutter
                } else {
                    start_x
                }
            }
        }
    }

    /// Compute the visible viewport regions for the current scroll state.
    ///
    /// Returns the pages and their visible rectangles in document-space points.
    pub fn compute_visible_regions(&self, state: &ViewportState) -> Vec<ViewportRegion> {
        let mut regions = Vec::new();

        // The visible area in document-space points.
        let vis_x = state.scroll_x;
        let vis_y = state.scroll_y;
        let vis_w = state.viewport_width / state.scale;
        let vis_h = state.viewport_height / state.scale;

        let page_count = self.page_count();
        if page_count == 0 {
            return regions;
        }

        for page in 0..page_count {
            let geom = &self.geometries[page as usize];
            let page_w = geom.effective_width();
            let page_h = geom.effective_height();

            let page_x = self.page_x_offset(
                page,
                state.layout,
                vis_w,
                state.facing_gutter,
            );
            let page_y = self.page_y_offset(
                page,
                state.layout,
                state.page_gap,
                state.facing_gutter,
            );

            // Check if this page intersects the visible area.
            let page_right = page_x + page_w;
            let page_bottom = page_y + page_h;

            if page_right <= vis_x || page_x >= vis_x + vis_w {
                continue; // Off-screen horizontally.
            }
            if page_bottom <= vis_y || page_y >= vis_y + vis_h {
                continue; // Off-screen vertically.
            }

            // Compute the visible rectangle within this page.
            let local_x = (vis_x - page_x).max(0.0) as u32;
            let local_y = (vis_y - page_y).max(0.0) as u32;
            let local_w = ((vis_x + vis_w - page_x).min(page_w) - local_x as f32).max(0.0) as u32;
            let local_h = ((vis_y + vis_h - page_y).min(page_h) - local_y as f32).max(0.0) as u32;

            if local_w > 0 && local_h > 0 {
                regions.push(ViewportRegion {
                    page,
                    x: local_x,
                    y: local_y,
                    w: local_w,
                    h: local_h,
                });
            }
        }

        regions
    }

    /// Build a complete `Viewport` from the current state, ready for the
    /// render scheduler.
    pub fn build_viewport(&self, state: &ViewportState) -> Viewport {
        let regions = self.compute_visible_regions(state);
        let (bucketed, _) = bucket_scale(state.scale);

        // Use the first visible page's dimensions for the tile grid.
        let (page_width, page_height) = if let Some(first) = regions.first() {
            let geom = &self.geometries[first.page as usize];
            (
                (geom.effective_width() * state.scale).ceil() as u32,
                (geom.effective_height() * state.scale).ceil() as u32,
            )
        } else {
            // Fallback: use A4-like dimensions.
            (
                (612.0 * state.scale).ceil() as u32,
                (792.0 * state.scale).ceil() as u32,
            )
        };

        Viewport {
            regions,
            scale: bucketed,
            rotation: state.rotation,
            page_width,
            page_height,
        }
    }

    /// Clamp the scroll position so the viewport doesn't go past the document.
    pub fn clamp_scroll(&self, state: &mut ViewportState) {
        let page_count = self.page_count();
        if page_count == 0 {
            state.scroll_y = 0.0;
            state.scroll_x = 0.0;
            return;
        }

        // Compute the total document height.
        let last_page = page_count - 1;
        let total_h = self.page_y_offset(
            last_page,
            state.layout,
            state.page_gap,
            state.facing_gutter,
        ) + self.geometries[last_page as usize].effective_height();

        let vis_h = state.viewport_height / state.scale;
        let max_scroll_y = (total_h - vis_h).max(0.0);
        state.scroll_y = state.scroll_y.clamp(0.0, max_scroll_y);
        state.scroll_x = state.scroll_x.clamp(0.0, 0.0); // Horizontal scroll TBD for wide pages.
    }

    /// Determine which page is "current" (the page most centered in the viewport).
    pub fn current_page(&self, state: &ViewportState) -> u32 {
        let page_count = self.page_count();
        if page_count == 0 {
            return 0;
        }

        let vis_center_y = state.scroll_y + (state.viewport_height / state.scale) / 2.0;

        let mut best_page = 0u32;
        let mut best_dist = f32::MAX;

        for page in 0..page_count {
            let page_y = self.page_y_offset(page, state.layout, state.page_gap, state.facing_gutter);
            let page_h = self.geometries[page as usize].effective_height();
            let page_center = page_y + page_h / 2.0;
            let dist = (vis_center_y - page_center).abs();
            if dist < best_dist {
                best_dist = dist;
                best_page = page;
            }
        }

        best_page
    }

    /// Scroll to a specific page (sets scroll_y to show that page at top).
    pub fn scroll_to_page(&self, page: u32, state: &mut ViewportState) {
        state.scroll_y = self.page_y_offset(page, state.layout, state.page_gap, state.facing_gutter);
        self.clamp_scroll(state);
    }

    /// Scroll by a number of pages (positive = down, negative = up).
    pub fn scroll_by_pages(&self, delta: i32, state: &mut ViewportState) {
        let current = self.current_page(state);
        let target = (current as i32 + delta).max(0) as u32;
        let target = target.min(self.page_count().saturating_sub(1));
        self.scroll_to_page(target, state);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn a4_geometry() -> PageGeometry {
        PageGeometry { width: 595.0, height: 842.0, rotation: 0 }
    }

    #[allow(dead_code)]
    fn letter_geometry() -> PageGeometry {
        PageGeometry { width: 612.0, height: 792.0, rotation: 0 }
    }

    #[allow(dead_code)]
    fn landscape_geometry() -> PageGeometry {
        PageGeometry { width: 842.0, height: 595.0, rotation: 0 }
    }

    #[test]
    fn page_geometry_effective_dimensions() {
        let portrait = PageGeometry { width: 100.0, height: 200.0, rotation: 0 };
        assert_eq!(portrait.effective_width(), 100.0);
        assert_eq!(portrait.effective_height(), 200.0);

        let rotated = PageGeometry { width: 100.0, height: 200.0, rotation: 90 };
        assert_eq!(rotated.effective_width(), 200.0);
        assert_eq!(rotated.effective_height(), 100.0);
    }

    #[test]
    fn scale_bucketing() {
        let (b, _) = bucket_scale(1.0);
        assert_eq!(b, 1.0);

        let (b, _) = bucket_scale(1.3);
        assert_eq!(b, 1.25);

        let (b, _) = bucket_scale(0.8);
        assert_eq!(b, 0.75);

        let (b, _) = bucket_scale(5.0);
        assert_eq!(b, 4.0);
    }

    #[test]
    fn continuous_layout_page_offsets() {
        let pos = PagePositioner::new(vec![a4_geometry(), a4_geometry(), a4_geometry()]);
        let gap = 20.0;
        let gutter = 0.0;

        // Page 0 starts at 0.
        assert_eq!(pos.page_y_offset(0, PageLayout::Continuous, gap, gutter), 0.0);

        // Page 1 starts after page 0 height + gap.
        assert_eq!(pos.page_y_offset(1, PageLayout::Continuous, gap, gutter), 842.0 + 20.0);

        // Page 2 starts after pages 0 and 1 + gaps.
        assert_eq!(pos.page_y_offset(2, PageLayout::Continuous, gap, gutter), (842.0 + 20.0) * 2.0);
    }

    #[test]
    fn single_layout_no_scroll_offset() {
        let pos = PagePositioner::new(vec![a4_geometry()]);
        let gap = 20.0;
        let gutter = 0.0;

        // In single mode, page always starts at 0.
        assert_eq!(pos.page_y_offset(0, PageLayout::Single, gap, gutter), 0.0);
    }

    #[test]
    fn facing_layout_pairs() {
        let pos = PagePositioner::new(vec![
            a4_geometry(), a4_geometry(),
            a4_geometry(), a4_geometry(),
        ]);
        let gap = 20.0;
        let gutter = 8.0;

        // Spread 0 (pages 0-1) starts at 0.
        assert_eq!(pos.page_y_offset(0, PageLayout::ContinuousFacing, gap, gutter), 0.0);
        assert_eq!(pos.page_y_offset(1, PageLayout::ContinuousFacing, gap, gutter), 0.0);

        // Spread 1 (pages 2-3) starts after spread 0 height + gap.
        let spread_h = 842.0; // Both pages same height.
        assert_eq!(pos.page_y_offset(2, PageLayout::ContinuousFacing, gap, gutter), spread_h + gap);
    }

    #[test]
    fn visible_regions_continuous() {
        let geos = vec![a4_geometry(), a4_geometry(), a4_geometry()];
        let pos = PagePositioner::new(geos);

        let mut state = ViewportState::new(800.0, 600.0);
        state.scale = 1.0;
        state.layout = PageLayout::Continuous;
        state.scroll_y = 0.0;

        let regions = pos.compute_visible_regions(&state);
        // Page 0 should be fully visible (842 > 600 viewport).
        assert!(regions.iter().any(|r| r.page == 0));
        // Page 1 should be partially visible if gap is small.
        // With gap=20, page 1 starts at 842+20=862, viewport goes to 600, so not visible.
        assert!(!regions.iter().any(|r| r.page == 1));
    }

    #[test]
    fn visible_regions_after_scroll() {
        let geos = vec![a4_geometry(), a4_geometry(), a4_geometry()];
        let pos = PagePositioner::new(geos);

        let mut state = ViewportState::new(800.0, 600.0);
        state.scale = 1.0;
        state.layout = PageLayout::Continuous;
        state.scroll_y = 900.0; // Scroll past page 0 into page 1.

        let regions = pos.compute_visible_regions(&state);
        // Page 1 starts at 862, viewport starts at 900, so page 1 is visible.
        assert!(regions.iter().any(|r| r.page == 1));
    }

    #[test]
    fn current_page_detection() {
        let geos = vec![a4_geometry(), a4_geometry(), a4_geometry()];
        let pos = PagePositioner::new(geos);

        let mut state = ViewportState::new(800.0, 600.0);
        state.scale = 1.0;
        state.layout = PageLayout::Continuous;

        state.scroll_y = 0.0;
        assert_eq!(pos.current_page(&state), 0);

        state.scroll_y = 900.0;
        assert_eq!(pos.current_page(&state), 1);
    }

    #[test]
    fn clamp_scroll_prevents_overscroll() {
        let geos = vec![a4_geometry()];
        let pos = PagePositioner::new(geos);

        let mut state = ViewportState::new(800.0, 600.0);
        state.scale = 1.0;
        state.layout = PageLayout::Continuous;
        state.scroll_y = 9999.0; // Way past the document.

        pos.clamp_scroll(&mut state);
        // Document is 842 tall, viewport 600, max scroll = 242.
        assert!((state.scroll_y - 242.0).abs() < 0.01);
    }

    #[test]
    fn build_viewport_produces_regions() {
        let geos = vec![a4_geometry(), a4_geometry()];
        let pos = PagePositioner::new(geos);

        let mut state = ViewportState::new(800.0, 600.0);
        state.scale = 1.0;
        state.layout = PageLayout::Continuous;

        let viewport = pos.build_viewport(&state);
        assert!(!viewport.regions.is_empty());
        assert_eq!(viewport.scale, 1.0);
    }

    #[test]
    fn zoom_by_center_keeps_center() {
        let mut state = ViewportState::new(800.0, 600.0);
        state.scroll_y = 100.0;
        state.scale = 1.0;

        let old_center_y = state.scroll_y + state.viewport_height / 2.0 / state.scale;
        state.zoom_by_center(2.0);
        let new_center_y = state.scroll_y + state.viewport_height / 2.0 / state.scale;

        // The document point at the center should stay in place.
        assert!((old_center_y - new_center_y).abs() < 0.01);
        assert!((state.scale - 2.0).abs() < f32::EPSILON);
    }

    #[test]
    fn zoom_by_focus_preserves_focal_point() {
        let mut state = ViewportState::new(800.0, 600.0);
        state.scale = 1.0;

        // Focus at (400, 300) — center of viewport.
        let focus_x = 400.0;
        let focus_y = 300.0;
        let doc_x_before = state.scroll_x + focus_x / state.scale;
        let doc_y_before = state.scroll_y + focus_y / state.scale;

        state.zoom_by(2.0, focus_x, focus_y);

        let doc_x_after = state.scroll_x + focus_x / state.scale;
        let doc_y_after = state.scroll_y + focus_y / state.scale;

        assert!((doc_x_before - doc_x_after).abs() < 0.01);
        assert!((doc_y_before - doc_y_after).abs() < 0.01);
    }

    #[test]
    fn zoom_clamps_to_bounds() {
        let mut state = ViewportState::new(800.0, 600.0);
        state.min_scale = 0.25;
        state.max_scale = 4.0;

        state.set_scale(0.1);
        assert_eq!(state.scale, 0.25);

        state.set_scale(10.0);
        assert_eq!(state.scale, 4.0);
    }

    #[test]
    fn zoom_to_fit_page() {
        let mut state = ViewportState::new(800.0, 600.0);
        state.zoom_to_fit_page(612.0, 792.0); // US Letter

        // Fit by height: 600/792 ≈ 0.757
        assert!((state.scale - 600.0 / 792.0).abs() < 0.01);
    }

    #[test]
    fn zoom_to_rect_centers_rectangle() {
        let mut state = ViewportState::new(800.0, 600.0);
        state.zoom_to_rect(100.0, 200.0, 400.0, 300.0);

        // Scale: min(800/400, 600/300) = 2.0
        assert!((state.scale - 2.0).abs() < 0.01);

        // Scroll should center the rectangle.
        // viewport_in_doc = 800/2 = 400, 600/2 = 300
        // scroll_x = 100 - (400 - 400)/2 = 100
        // scroll_y = 200 - (300 - 300)/2 = 200
        assert!((state.scroll_x - 100.0).abs() < 0.01);
        assert!((state.scroll_y - 200.0).abs() < 0.01);
    }

    #[test]
    fn zoom_in_out_step() {
        let mut state = ViewportState::new(800.0, 600.0);
        state.scale = 1.0;

        state.zoom_in_step();
        assert!(state.scale > 1.0);
        let after_in = state.scale;

        state.zoom_out_step();
        assert!(state.scale < after_in);
    }

    #[test]
    fn zoom_to_actual_size() {
        let mut state = ViewportState::new(800.0, 600.0);
        state.scale = 2.5;
        state.zoom_to_actual_size();
        assert!((state.scale - 1.0).abs() < f32::EPSILON);
    }
}
