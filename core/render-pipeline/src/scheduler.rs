//! Tile scheduler: viewport → tile requests → priority queue → worker dispatch. [ADR-007, SDS §6]
//!
//! Decomposes a viewport (pages × rects × scale × rotation) into fixed-size
//! device-space tiles, assigns priorities, and manages generation counters
//! for invalidation.

use protocol::handles::TILE_EDGE_PX;

/// A tile coordinate within a page (device-space grid position).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TileCoord {
    /// Column index (0-based) in the tile grid.
    pub col: u32,
    /// Row index (0-based) in the tile grid.
    pub row: u32,
}

/// A tile request: one tile to render on one page.
#[derive(Debug, Clone)]
pub struct TileRequest {
    /// 0-based page index.
    pub page: u32,
    /// Tile grid position.
    pub coord: TileCoord,
    /// Device-space origin of this tile (pixels from page origin).
    pub x: u32,
    /// Device-space origin of this tile.
    pub y: u32,
    /// Tile width (typically TILE_EDGE_PX, less for edge tiles).
    pub w: u32,
    /// Tile height (typically TILE_EDGE_PX, less for edge tiles).
    pub h: u32,
    /// Render scale.
    pub scale: f32,
    /// Priority (lower = more urgent).
    pub priority: u8,
    /// Generation for invalidation.
    pub generation: u64,
}

/// Priority levels for tile requests. [SDS §6.2]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    /// Currently visible in the viewport.
    Visible = 0,
    /// In the prefetch ring (near viewport).
    Prefetch = 1,
    /// Thumbnail for panel.
    Thumbnail = 2,
    /// Background (index, optimization).
    Background = 3,
}

impl Priority {
    /// Convert to numeric for sorting.
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

/// A viewport region: one page's visible area.
#[derive(Debug, Clone)]
pub struct ViewportRegion {
    /// 0-based page index.
    pub page: u32,
    /// Visible rectangle in page device-space coordinates.
    pub x: u32,
    /// Top edge of visible region.
    pub y: u32,
    /// Width of visible region.
    pub w: u32,
    /// Height of visible region.
    pub h: u32,
}

/// A complete viewport: set of visible page regions + display parameters.
#[derive(Debug, Clone)]
pub struct Viewport {
    /// Visible page regions.
    pub regions: Vec<ViewportRegion>,
    /// Current scale factor.
    pub scale: f32,
    /// Rotation in degrees (0, 90, 180, 270).
    pub rotation: u32,
    /// Page width in device pixels (at current scale). Used for tile grid.
    pub page_width: u32,
    /// Page height in device pixels (at current scale).
    pub page_height: u32,
}

impl Viewport {
    /// Decompose this viewport into tile requests with priorities.
    ///
    /// Returns all tiles that should be rendered, sorted by priority.
    pub fn decompose(&self, generation: u64) -> Vec<TileRequest> {
        let mut requests = Vec::new();
        let edge = TILE_EDGE_PX;

        for region in &self.regions {
            // Compute the tile grid covering this region.
            let start_col = region.x / edge;
            let end_col = (region.x + region.w).saturating_sub(1) / edge;
            let start_row = region.y / edge;
            let end_row = (region.y + region.h).saturating_sub(1) / edge;

            for row in start_row..=end_row {
                for col in start_col..=end_col {
                    let tile_x = col * edge;
                    let tile_y = row * edge;
                    let tile_w = self.page_width.saturating_sub(tile_x).min(edge);
                    let tile_h = self.page_height.saturating_sub(tile_y).min(edge);

                    if tile_w == 0 || tile_h == 0 {
                        continue;
                    }

                    requests.push(TileRequest {
                        page: region.page,
                        coord: TileCoord { col, row },
                        x: tile_x,
                        y: tile_y,
                        w: tile_w,
                        h: tile_h,
                        scale: self.scale,
                        priority: Priority::Visible.as_u8(),
                        generation,
                    });
                }
            }
        }

        // Sort by priority (visible first).
        requests.sort_by_key(|r| r.priority);
        requests
    }

    /// Compute a prefetch ring around the visible regions.
    /// Adds tiles adjacent to visible tiles with prefetch priority.
    pub fn decompose_with_prefetch(
        &self,
        generation: u64,
        prefetch_margin: u32,
    ) -> Vec<TileRequest> {
        let mut requests = self.decompose(generation);
        let visible: std::collections::HashSet<(u32, u32, u32)> = requests
            .iter()
            .map(|r| (r.page, r.coord.col, r.coord.row))
            .collect();
        let edge = TILE_EDGE_PX;

        for region in &self.regions {
            let start_col = region.x / edge;
            let end_col = (region.x + region.w).saturating_sub(1) / edge;
            let start_row = region.y / edge;
            let end_row = (region.y + region.h).saturating_sub(1) / edge;

            // Extend by prefetch margin.
            let pf_start_col = start_col.saturating_sub(prefetch_margin);
            let pf_end_col = end_col + prefetch_margin;
            let pf_start_row = start_row.saturating_sub(prefetch_margin);
            let pf_end_row = end_row + prefetch_margin;

            for row in pf_start_row..=pf_end_row {
                for col in pf_start_col..=pf_end_col {
                    if visible.contains(&(region.page, col, row)) {
                        continue;
                    }

                    let tile_x = col * edge;
                    let tile_y = row * edge;
                    let tile_w = self.page_width.saturating_sub(tile_x).min(edge);
                    let tile_h = self.page_height.saturating_sub(tile_y).min(edge);

                    if tile_w == 0 || tile_h == 0 {
                        continue;
                    }

                    requests.push(TileRequest {
                        page: region.page,
                        coord: TileCoord { col, row },
                        x: tile_x,
                        y: tile_y,
                        w: tile_w,
                        h: tile_h,
                        scale: self.scale,
                        priority: Priority::Prefetch.as_u8(),
                        generation,
                    });
                }
            }
        }

        requests.sort_by_key(|r| r.priority);
        requests
    }
}

/// Render scheduler: tracks generations, decomposes viewports, deduplicates requests.
#[derive(Debug)]
pub struct RenderScheduler {
    /// Current generation counter.
    generation: u64,
    /// Set of (page, col, row) currently in-flight.
    in_flight: std::collections::HashSet<(u32, u32, u32)>,
}

impl RenderScheduler {
    /// Create a new scheduler.
    pub fn new() -> Self {
        Self {
            generation: 0,
            in_flight: std::collections::HashSet::new(),
        }
    }

    /// Increment the generation (called when the document changes).
    pub fn bump_generation(&mut self) -> u64 {
        self.generation += 1;
        // Clear in-flight on generation bump — stale requests will be dropped.
        self.in_flight.clear();
        self.generation
    }

    /// Get the current generation.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Process a viewport and return new tile requests (deduplicating in-flight).
    ///
    /// `prefetch_margin` controls how many extra tiles beyond the visible area
    /// to prefetch. Use velocity-aware margins for fast scrolling (wider margin)
    /// and tight margins for slow/zoom interaction.
    pub fn schedule_viewport(&mut self, viewport: &Viewport, prefetch_margin: u32) -> Vec<TileRequest> {
        let requests = viewport.decompose_with_prefetch(self.generation, prefetch_margin);
        let mut new_requests = Vec::new();

        for req in requests {
            let key = (req.page, req.coord.col, req.coord.row);
            if self.in_flight.insert(key) {
                new_requests.push(req);
            }
        }

        new_requests
    }

    /// Mark a tile as completed (remove from in-flight).
    pub fn mark_completed(&mut self, page: u32, col: u32, row: u32) {
        self.in_flight.remove(&(page, col, row));
    }

    /// Cancel all in-flight requests (e.g., on document change).
    pub fn cancel_all(&mut self) {
        self.in_flight.clear();
    }

    /// Number of tiles currently in-flight.
    pub fn in_flight_count(&self) -> usize {
        self.in_flight.len()
    }
}

impl Default for RenderScheduler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewport_decompose_basic() {
        let vp = Viewport {
            regions: vec![ViewportRegion { page: 0, x: 0, y: 0, w: 256, h: 256 }],
            scale: 1.0,
            rotation: 0,
            page_width: 612, // US Letter at 72 DPI
            page_height: 792,
        };
        let requests = vp.decompose(1);
        assert_eq!(requests.len(), 1); // One 256x256 tile covers the region
        assert_eq!(requests[0].page, 0);
        assert_eq!(requests[0].coord, TileCoord { col: 0, row: 0 });
        assert_eq!(requests[0].generation, 1);
    }

    #[test]
    fn viewport_decompose_large_region() {
        let vp = Viewport {
            regions: vec![ViewportRegion { page: 0, x: 0, y: 0, w: 512, h: 512 }],
            scale: 1.0,
            rotation: 0,
            page_width: 612,
            page_height: 792,
        };
        let requests = vp.decompose(1);
        // 512/256 = 2 columns, 512/256 = 2 rows = 4 tiles
        assert_eq!(requests.len(), 4);
    }

    #[test]
    fn scheduler_deduplicates() {
        let mut sched = RenderScheduler::new();
        let vp = Viewport {
            regions: vec![ViewportRegion { page: 0, x: 0, y: 0, w: 256, h: 256 }],
            scale: 1.0,
            rotation: 0,
            page_width: 612,
            page_height: 792,
        };

        let r1 = sched.schedule_viewport(&vp, 2);
        let count = r1.len();
        assert!(count > 0, "should produce tile requests");

        // Same viewport — should produce no new requests (all in-flight).
        let r2 = sched.schedule_viewport(&vp, 2);
        assert_eq!(r2.len(), 0);
    }

    #[test]
    fn scheduler_generation_bump_cancels() {
        let mut sched = RenderScheduler::new();
        let vp = Viewport {
            regions: vec![ViewportRegion { page: 0, x: 0, y: 0, w: 256, h: 256 }],
            scale: 1.0,
            rotation: 0,
            page_width: 612,
            page_height: 792,
        };

        sched.schedule_viewport(&vp, 2);
        let count = sched.in_flight_count();
        assert!(count > 0, "should have in-flight tiles");

        let gen = sched.bump_generation();
        assert_eq!(gen, 1);
        assert_eq!(sched.in_flight_count(), 0);
    }

    #[test]
    fn prefetch_adds_surrounding_tiles() {
        let vp = Viewport {
            regions: vec![ViewportRegion { page: 0, x: 256, y: 256, w: 256, h: 256 }],
            scale: 1.0,
            rotation: 0,
            page_width: 1024,
            page_height: 1024,
        };
        let requests = vp.decompose_with_prefetch(1, 1);
        // Visible: 1 tile at (1,1). Prefetch: 8 surrounding tiles in 3x3 grid.
        assert!(requests.len() >= 9, "expected >= 9 tiles, got {}", requests.len());
        // First should be visible priority.
        assert_eq!(requests[0].priority, Priority::Visible.as_u8());
    }
}
