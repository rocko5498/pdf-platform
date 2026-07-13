//! Render coordination: viewport → scheduler → worker dispatch → tile cache. [SDS §6, ADR-007]
//!
//! Wires the render scheduler to the worker session, dispatches tile requests
//! with slot allocation from a TilePool, and receives TILE_READY responses.
//! The tile cache holds rendered descriptors for the shell to read from shmem.

use std::collections::HashMap;
use std::time::Duration;

use protocol::commands::{encode_render_tile, RenderTileRequest};
use protocol::handles::{decode_tile_ready, TileSlotDesc, TILE_RGBA8_BYTES};
use render_pipeline::cache::{CacheEntry, TileCache, TileCacheKey};
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
    key: TileKey,
    generation: u64,
}

/// Render coordinator: manages scheduling, dispatch, and tile cache.
pub struct RenderLoop {
    scheduler: RenderScheduler,
    cache: TileCache,
    pool: TilePool,
    /// Maps slot_offset → pending request info for correlating TILE_READY responses.
    pending: HashMap<u32, PendingRequest>,
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
        }
    }

    /// Process a viewport change: decompose into tiles, dispatch to worker,
    /// and return any completed tile descriptors.
    ///
    /// This is the main render loop iteration. Call it when:
    /// - The shell publishes a new viewport (scroll/zoom/rotate)
    /// - The document changes (bump_generation first)
    pub fn update_viewport(
        &mut self,
        session: &mut WorkerSession,
        viewport: &Viewport,
    ) -> Result<Vec<(TileKey, TileSlotDesc)>, SessionError> {
        // Get new tile requests from the scheduler.
        let requests = self.scheduler.schedule_viewport(viewport);

        // Dispatch each request to the worker.
        for req in &requests {
            self.dispatch_tile(session, req)?;
        }

        // Poll for completed tiles (non-blocking).
        self.poll_completed(session)
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
        let (slot_index, slot_offset) = match self.pool.alloc_slot(gen) {
            Some(alloc) => alloc,
            None => {
                // Pool exhausted — skip this tile for now; it will be retried
                // on the next viewport update when slots free up.
                return Ok(());
            }
        };

        let cmd = RenderTileRequest {
            page: req.page,
            x: req.x,
            y: req.y,
            w: req.w,
            h: req.h,
            scale: req.scale,
            generation: req.generation,
            slot_offset,
        };

        // Track the pending request for correlation.
        self.pending.insert(slot_offset, PendingRequest { key, generation: gen });

        let body = encode_render_tile(&cmd);
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
                    if let Ok(desc) = decode_tile_ready(&frame) {
                        let key = TileKey { page: desc.page, col: desc.col, row: desc.row };

                        // Release the pool slot.
                        self.pool.mark_ready(desc.offset as usize / TILE_RGBA8_BYTES);

                        // Remove from pending map (cleanup).
                        self.pending.remove(&desc.offset);

                        self.scheduler.mark_completed(key.page, key.col, key.row);
                        self.cache.insert(
                            key.to_cache_key(),
                            CacheEntry {
                                desc: desc.clone(),
                                generation: desc.generation,
                            },
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
