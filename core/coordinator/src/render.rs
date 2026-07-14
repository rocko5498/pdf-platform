//! Render coordination: viewport → scheduler → worker dispatch → tile cache. [SDS §6, ADR-007]
//!
//! Wires the render scheduler to the worker session, dispatches tile requests
//! with slot allocation from a TilePool, and receives TILE_READY responses.
//! The tile cache holds rendered descriptors for the shell to read from shmem.
//!
//! The `RenderLoop` owns the full pipeline: viewport state → page positioning →
//! tile scheduling → dispatch → cache. The shell interacts through
//! `update_viewport_state()` which handles the entire cycle in one call.

use std::collections::HashMap;
use std::time::Duration;

use protocol::commands::{encode_command, Command};
use protocol::events::{decode_worker_event, WorkerEvent};
use protocol::handles::{decode_tile_ready, TileSlotDesc, TILE_RGBA8_BYTES};
use render_pipeline::cache::{CacheEntry, TileCache, TileCacheKey};
use render_pipeline::layout::{
    compute_prefetch_margin, PageGeometry, PagePositioner, ViewportState,
};
use render_pipeline::scheduler::{RenderScheduler, TileRequest, Viewport};
use render_pipeline::shmem::TilePool;

use crate::session::{SessionError, WorkerSession};

/// Key for a cached tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TileKey {
    /// 0-based page index.
    pub page: u32,
    /// Column in tile grid.
    pub col: u32,
    /// Row in tile grid.
    pub row: u32,
}

impl TileKey {
    /// Convert to a cache key (same fields, different type for the cache module).
    fn to_cache_key(self) -> TileCacheKey {
        TileCacheKey { page: self.page, col: self.col, row: self.row }
    }
}

/// A cached tile: descriptor + validity.
#[derive(Debug, Clone)]
pub struct CachedTile {
    /// Descriptor for reading pixels from shmem.
    pub desc: TileSlotDesc,
    /// Generation when this tile was rendered.
    pub generation: u64,
}

/// Tracks an in-flight render request so we can map slot_offset back to TileKey.
#[derive(Debug, Clone)]
struct PendingRequest {
    #[allow(dead_code)] // reserved for diagnostics / render-quality tracking
    key: TileKey,
    #[allow(dead_code)] // reserved for generation-based stale-request logging
    generation: u64,
}

/// Render coordinator: manages scheduling, dispatch, and tile cache.
///
/// Owns the full pipeline from viewport state to rendered tiles. The shell
/// interacts through [`update_viewport_state`] which handles the entire
/// cycle: state → viewport → scheduling → dispatch → cache.
pub struct RenderLoop {
    scheduler: RenderScheduler,
    cache: TileCache,
    pool: TilePool,
    /// Maps slot_offset → pending request info for correlating TILE_READY responses.
    pending: HashMap<u32, PendingRequest>,
    /// Page positioner: computes page positions from viewport state.
    positioner: PagePositioner,
    /// Current viewport state (scroll, zoom, layout).
    state: ViewportState,
}

impl RenderLoop {
    /// Create a new render loop with the given tile pool size.
    ///
    /// `num_slots` controls how many tiles can be in-flight or cached simultaneously.
    /// `cache_max_bytes` is the LRU eviction budget.
    pub fn new(num_slots: usize, cache_max_bytes: usize) -> Self {
        let pool = TilePool::create(num_slots).expect("failed to create tile pool");
        Self {
            scheduler: RenderScheduler::new(),
            cache: TileCache::new(cache_max_bytes, TILE_RGBA8_BYTES),
            pool,
            pending: HashMap::new(),
            positioner: PagePositioner::new(Vec::new()),
            state: ViewportState::new(800.0, 600.0),
        }
    }

    /// Set the page geometries for the document.
    ///
    /// Must be called after opening a document and before rendering.
    pub fn set_page_geometries(&mut self, geometries: Vec<PageGeometry>) {
        self.positioner = PagePositioner::new(geometries);
    }

    /// Set the initial viewport state (e.g. after opening a document).
    pub fn set_viewport_state(&mut self, state: ViewportState) {
        self.state = state;
    }

    /// Get a reference to the current viewport state.
    pub fn viewport_state(&self) -> &ViewportState {
        &self.state
    }

    /// Get a mutable reference to the viewport state for the shell to modify.
    pub fn viewport_state_mut(&mut self) -> &mut ViewportState {
        &mut self.state
    }

    /// Get the page positioner.
    pub fn positioner(&self) -> &PagePositioner {
        &self.positioner
    }

    /// The main render loop iteration driven by viewport state.
    ///
    /// Call this whenever the user scrolls, zooms, changes layout, or when
    /// the document changes. It:
    /// 1. Computes the viewport from the current state
    /// 2. Determines the prefetch margin from scroll velocity
    /// 3. Schedules tile requests (with deduplication)
    /// 4. Dispatches new tiles to the worker
    /// 5. Polls for completed tiles
    /// 6. Returns all tiles that just completed
    ///
    /// If the worker has died, it automatically respawns. [SDS §10.1]
    pub fn update_viewport_state(
        &mut self,
        session: &mut WorkerSession,
    ) -> Result<Vec<(TileKey, TileSlotDesc)>, SessionError> {
        // Build the viewport from the current state.
        let viewport = self.positioner.build_viewport(&self.state);

        // Compute velocity-aware prefetch margin.
        let viewport_height_tiles =
            self.state.viewport_height / (TILE_RGBA8_BYTES as f32).sqrt();
        let prefetch_margin = compute_prefetch_margin(
            self.state.scroll_velocity_y,
            viewport_height_tiles,
            2, // min_margin
            8, // max_margin
        );

        self.update_viewport_with_margin(session, &viewport, prefetch_margin)
    }

    /// Process a viewport change with an explicit prefetch margin.
    ///
    /// This is the lower-level method that accepts a pre-built viewport.
    /// Prefer [`update_viewport_state`] for the standard path.
    pub fn update_viewport_with_margin(
        &mut self,
        session: &mut WorkerSession,
        viewport: &Viewport,
        prefetch_margin: u32,
    ) -> Result<Vec<(TileKey, TileSlotDesc)>, SessionError> {
        // Check for worker death and respawn if needed.
        self.handle_worker_death(session)?;

        // Get new tile requests from the scheduler.
        let requests = self.scheduler.schedule_viewport(viewport, prefetch_margin);

        // Dispatch each request to the worker.
        for req in &requests {
            self.dispatch_tile(session, req)?;
        }

        // Poll for completed tiles (non-blocking).
        self.poll_completed(session)
    }

    /// Check if the worker is dead and respawn it. [SDS §10.1]
    ///
    /// After respawn, any pending tile requests remain valid — they will be
    /// re-dispatched on the next `update_viewport` call since the scheduler
    /// still considers them in-flight.
    fn handle_worker_death(&mut self, session: &mut WorkerSession) -> Result<(), SessionError> {
        if session.is_alive() {
            return Ok(());
        }

        // Worker is dead — clear pending requests (they used the old worker's
        // shmem slots which are now invalid) and release pool slots.
        for (_slot_offset, _req) in self.pending.drain() {
            // Pool slots are automatically reclaimable since the worker is dead.
        }

        // Respawn the worker.
        session.respawn()?;

        // Invalidate the cache since the respawned worker has no state.
        self.cache.clear();
        self.pool.invalidate_up_to(self.scheduler.generation());

        Ok(())
    }

    /// Notify the render loop that the document has changed.
    /// Invalidates all cached tiles and bumps the generation.
    pub fn invalidate(&mut self) -> u64 {
        let gen = self.scheduler.bump_generation();
        self.cache.clear();
        self.pool.invalidate_up_to(gen);
        gen
    }

    /// Dispatch a single tile request to the worker.
    ///
    /// Allocates a slot from the pool and tracks the mapping from slot_offset
    /// to TileKey so we can correlate TILE_READY responses.
    fn dispatch_tile(
        &mut self,
        session: &mut WorkerSession,
        req: &TileRequest,
    ) -> Result<(), SessionError> {
        let gen = req.generation;
        let key = TileKey { page: req.page, col: req.coord.col, row: req.coord.row };

        // Check if this tile is already in the cache at the same generation.
        let cache_key = key.to_cache_key();
        if let Some(entry) = self.cache.get(&cache_key) {
            if entry.generation == gen {
                // Already cached at current generation, skip dispatch.
                self.scheduler.mark_completed(req.page, req.coord.col, req.coord.row);
                return Ok(());
            }
        }

        // Allocate a slot from the pool.
        let (_slot_index, slot_offset) = match self.pool.alloc_slot(gen) {
            Some(alloc) => alloc,
            None => {
                // Pool exhausted — skip this tile for now; it will be retried
                // on the next viewport update when slots free up.
                return Ok(());
            }
        };

        let correlation_id = session.next_correlation_id();

        // Track the pending request for correlation.
        self.pending.insert(slot_offset, PendingRequest { key, generation: gen });

        let cmd = Command::RenderTile {
            correlation_id,
            page: req.page,
            x: req.x,
            y: req.y,
            w: req.w,
            h: req.h,
            scale: req.scale,
            generation: req.generation,
            slot_offset,
            col: req.coord.col,
            row: req.coord.row,
        };
        let body = encode_command(&cmd);
        session.send(&body)
    }

    /// Poll the worker for completed TILE_READY responses.
    ///
    /// Uses tile identity (page/col/row) from the descriptor to correlate
    /// with the scheduler, and slot_offset to release the pool slot.
    fn poll_completed(
        &mut self,
        session: &mut WorkerSession,
    ) -> Result<Vec<(TileKey, TileSlotDesc)>, SessionError> {
        let mut completed = Vec::new();

        // Non-blocking poll: read all available frames.
        loop {
            match session.recv_frame(Duration::from_millis(0)) {
                Ok(frame) => {
                    // Try typed event decode first.
                    if let Ok(WorkerEvent::TileReady { desc, .. }) = decode_worker_event(&frame) {
                        let key = TileKey { page: desc.page, col: desc.col, row: desc.row };
                        self.pool.mark_ready(desc.offset as usize / TILE_RGBA8_BYTES);
                        self.pending.remove(&desc.offset);
                        self.scheduler.mark_completed(key.page, key.col, key.row);
                        self.cache.insert(
                            key.to_cache_key(),
                            CacheEntry { desc: desc.clone(), generation: desc.generation },
                        );
                        completed.push((key, desc));
                        continue;
                    }
                    // Fall back to legacy TILE_READY codec.
                    if let Ok(desc) = decode_tile_ready(&frame) {
                        let key = TileKey { page: desc.page, col: desc.col, row: desc.row };
                        self.pool.mark_ready(desc.offset as usize / TILE_RGBA8_BYTES);
                        self.pending.remove(&desc.offset);
                        self.scheduler.mark_completed(key.page, key.col, key.row);
                        self.cache.insert(
                            key.to_cache_key(),
                            CacheEntry { desc: desc.clone(), generation: desc.generation },
                        );
                        completed.push((key, desc));
                    }
                }
                Err(_) => break, // No more frames available.
            }
        }

        Ok(completed)
    }

    /// Look up a cached tile. Promotes to most-recently-used on hit.
    pub fn get_tile(&mut self, key: &TileKey) -> Option<CachedTile> {
        self.cache.get(&key.to_cache_key()).map(|e| CachedTile {
            desc: e.desc.clone(),
            generation: e.generation,
        })
    }

    /// Number of tiles currently in the cache.
    pub fn cache_size(&self) -> usize {
        self.cache.len()
    }

    /// Total bytes used by cached tiles.
    pub fn cache_bytes(&self) -> usize {
        self.cache.current_bytes()
    }

    /// Number of tiles currently in-flight (dispatched but not yet completed).
    pub fn in_flight_count(&self) -> usize {
        self.scheduler.in_flight_count()
    }

    /// Get the current generation.
    pub fn generation(&self) -> u64 {
        self.scheduler.generation()
    }

    /// Get all currently cached tiles that are visible in the viewport.
    ///
    /// Returns tile keys and their descriptors, sorted by page then position.
    /// The shell uses this to composite tiles onto the canvas.
    pub fn get_visible_tiles(&mut self) -> Vec<(TileKey, CachedTile)> {
        let regions = self.positioner.compute_visible_regions(&self.state);
        let mut result = Vec::new();

        for region in &regions {
            let tile_edge = 256u32; // TILE_EDGE_PX
            let start_col = region.x / tile_edge;
            let end_col = (region.x + region.w).saturating_sub(1) / tile_edge;
            let start_row = region.y / tile_edge;
            let end_row = (region.y + region.h).saturating_sub(1) / tile_edge;

            for row in start_row..=end_row {
                for col in start_col..=end_col {
                    let key = TileKey { page: region.page, col, row };
                    if let Some(entry) = self.cache.get(&key.to_cache_key()) {
                        result.push((key, CachedTile {
                            desc: entry.desc.clone(),
                            generation: entry.generation,
                        }));
                    }
                }
            }
        }

        result
    }

    /// Get the 0-based index of the page most centered in the viewport.
    pub fn current_page(&self) -> u32 {
        self.positioner.current_page(&self.state)
    }

    /// Get the total number of pages in the document.
    pub fn page_count(&self) -> u32 {
        self.positioner.page_count()
    }
}

impl Default for RenderLoop {
    fn default() -> Self {
        // Default: 4 slots, 4 tiles worth of cache (1 MB).
        Self::new(4, TILE_RGBA8_BYTES * 4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_loop_invalidate_bumps_generation() {
        let mut rl = RenderLoop::default();
        assert_eq!(rl.generation(), 0);
        let gen = rl.invalidate();
        assert_eq!(gen, 1);
        assert_eq!(rl.generation(), 1);
        assert_eq!(rl.cache_size(), 0);
    }

    #[test]
    fn render_loop_cache_accounting() {
        let mut rl = RenderLoop::default();
        let key = TileKey { page: 0, col: 0, row: 0 };
        let desc = TileSlotDesc {
            offset: 0,
            len: TILE_RGBA8_BYTES as u32,
            format: protocol::handles::PixelFormat::Rgba8,
            generation: 1,
            page: 0,
            col: 0,
            row: 0,
        };
        rl.cache.insert(
            key.to_cache_key(),
            CacheEntry { desc, generation: 1 },
        );
        assert_eq!(rl.cache_size(), 1);
        assert_eq!(rl.cache_bytes(), TILE_RGBA8_BYTES);
    }

    #[test]
    fn render_loop_get_tile() {
        let mut rl = RenderLoop::default();
        let key = TileKey { page: 0, col: 1, row: 2 };
        assert!(rl.get_tile(&key).is_none());

        let desc = TileSlotDesc {
            offset: 0,
            len: TILE_RGBA8_BYTES as u32,
            format: protocol::handles::PixelFormat::Rgba8,
            generation: 1,
            page: 0,
            col: 1,
            row: 2,
        };
        rl.cache.insert(
            key.to_cache_key(),
            CacheEntry { desc, generation: 1 },
        );
        assert!(rl.get_tile(&key).is_some());
    }

    #[test]
    fn render_loop_default_pool_size() {
        let rl = RenderLoop::default();
        assert_eq!(rl.pool.slot_count(), 4);
    }
}
