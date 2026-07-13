//! Render coordination: viewport → scheduler → worker dispatch → tile cache. [SDS §6, ADR-007]
//!
//! M1: wires the render scheduler to the worker session, dispatches tile requests,
//! and receives TILE_READY responses. The tile cache holds rendered descriptors
//! for the shell to read from shmem.

use std::collections::HashMap;
use std::time::Duration;

use protocol::commands::{encode_render_tile, RenderTileRequest};
use protocol::handles::{decode_tile_ready, TileSlotDesc, TILE_RGBA8_BYTES};
use protocol::transport::WorkerTransport as _;
use render_pipeline::scheduler::{RenderScheduler, TileRequest, Viewport};

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

/// A cached tile: descriptor + validity.
#[derive(Debug, Clone)]
pub struct CachedTile {
    /// Descriptor for reading pixels from shmem.
    pub desc: TileSlotDesc,
    /// Generation when this tile was rendered.
    pub generation: u64,
}

/// Render coordinator: manages scheduling, dispatch, and tile cache.
pub struct RenderLoop {
    scheduler: RenderScheduler,
    /// Tiles that have been rendered and are valid.
    cache: HashMap<TileKey, CachedTile>,
    /// Bytes per tile (for memory accounting).
    tile_bytes: usize,
}

impl RenderLoop {
    /// Create a new render loop.
    pub fn new() -> Self {
        Self {
            scheduler: RenderScheduler::new(),
            cache: HashMap::new(),
            tile_bytes: TILE_RGBA8_BYTES,
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
        gen
    }

    /// Dispatch a single tile request to the worker.
    fn dispatch_tile(
        &self,
        session: &mut WorkerSession,
        req: &TileRequest,
    ) -> Result<(), SessionError> {
        let cmd = RenderTileRequest {
            page: req.page,
            x: req.x,
            y: req.y,
            w: req.w,
            h: req.h,
            scale: req.scale,
            generation: req.generation,
            slot_offset: 0, // M0: single slot
        };
        let body = encode_render_tile(&cmd);
        session.send(&body)
    }

    /// Poll the worker for completed TILE_READY responses.
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
                        // For M0 single-slot: we don't know which tile this is
                        // from the descriptor alone. We track by completion order.
                        // TODO: encode tile key in the TILE_READY frame.
                        let key = TileKey { page: 0, col: 0, row: 0 };
                        self.scheduler.mark_completed(key.page, key.col, key.row);
                        self.cache.insert(
                            key,
                            CachedTile {
                                desc: desc.clone(),
                                generation: self.scheduler.generation(),
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

    /// Look up a cached tile.
    pub fn get_tile(&self, key: &TileKey) -> Option<&CachedTile> {
        self.cache.get(key)
    }

    /// Number of tiles currently in the cache.
    pub fn cache_size(&self) -> usize {
        self.cache.len()
    }

    /// Total bytes used by cached tiles.
    pub fn cache_bytes(&self) -> usize {
        self.cache.len() * self.tile_bytes
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
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use render_pipeline::scheduler::{ViewportRegion, Viewport};

    #[test]
    fn render_loop_invalidate_bumps_generation() {
        let mut rl = RenderLoop::new();
        assert_eq!(rl.generation(), 0);
        let gen = rl.invalidate();
        assert_eq!(gen, 1);
        assert_eq!(rl.generation(), 1);
        assert_eq!(rl.cache_size(), 0);
    }

    #[test]
    fn render_loop_cache_accounting() {
        let mut rl = RenderLoop::new();
        let key = TileKey { page: 0, col: 0, row: 0 };
        let desc = TileSlotDesc {
            offset: 0,
            len: TILE_RGBA8_BYTES as u32,
            format: protocol::handles::PixelFormat::Rgba8,
            generation: 1,
        };
        rl.cache.insert(key, CachedTile { desc, generation: 1 });
        assert_eq!(rl.cache_size(), 1);
        assert_eq!(rl.cache_bytes(), TILE_RGBA8_BYTES);
    }

    #[test]
    fn render_loop_get_tile() {
        let mut rl = RenderLoop::new();
        let key = TileKey { page: 0, col: 1, row: 2 };
        assert!(rl.get_tile(&key).is_none());

        let desc = TileSlotDesc {
            offset: 0,
            len: TILE_RGBA8_BYTES as u32,
            format: protocol::handles::PixelFormat::Rgba8,
            generation: 1,
        };
        rl.cache.insert(key, CachedTile { desc, generation: 1 });
        assert!(rl.get_tile(&key).is_some());
    }
}
