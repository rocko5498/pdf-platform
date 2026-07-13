//! Shared-memory tile pool: pre-allocated slots for cross-process pixel transfer. [ADR-007, SDS §6]
//!
//! The pool wraps a `sandbox::shmem::SharedRegion` and sub-divides it into fixed-size
//! tile slots. The coordinator allocates slots; the worker writes pixels into them;
//! the coordinator reads them back after `TILE_READY`. [GR-7: bounded, eviction policy]

use std::fs::File;

use protocol::handles::TILE_RGBA8_BYTES;
use sandbox::shmem::{map_shmem_file, SharedRegion};

/// A tile slot's tracking state.
#[derive(Debug, Clone)]
struct SlotState {
    /// Generation when this slot was allocated. Stale if mismatched.
    generation: u64,
    /// Whether this slot is currently allocated to a pending render.
    allocated: bool,
}

/// Pre-allocated pool of tile slots in shared memory. [GR-7]
///
/// Each slot is `TILE_RGBA8_BYTES` (256×256×4 = 262,144 bytes). The pool
/// manages allocation, generation tracking, and invalidation.
pub struct TilePool {
    region: SharedRegion,
    slot_size: usize,
    slots: Vec<SlotState>,
}

impl std::fmt::Debug for TilePool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TilePool")
            .field("slot_size", &self.slot_size)
            .field("slots", &self.slots)
            .finish()
    }
}

impl TilePool {
    /// Create a pool with the given number of slots.
    ///
    /// Allocates `num_slots * TILE_RGBA8_BYTES` of shared memory.
    pub fn create(num_slots: usize) -> Result<Self, String> {
        assert!(num_slots > 0, "pool must have at least 1 slot");
        let total = num_slots * TILE_RGBA8_BYTES;
        let region = SharedRegion::create(total).map_err(|e| e.to_string())?;
        let slots = (0..num_slots)
            .map(|_| SlotState { generation: 0, allocated: false })
            .collect();
        Ok(Self { region, slot_size: TILE_RGBA8_BYTES, slots })
    }

    /// Get the file handle for inheriting into a worker process.
    pub fn file(&self) -> &File {
        self.region.file()
    }

    /// Total byte length of the shared region.
    pub fn len(&self) -> usize {
        self.region.len()
    }

    /// Number of slots in the pool.
    pub fn slot_count(&self) -> usize {
        self.slots.len()
    }

    /// Allocate a slot for a new render. Returns `(slot_index, byte_offset)`.
    ///
    /// Reuses stale or unallocated slots. Returns `None` if all slots are
    /// actively allocated to newer generations.
    pub fn alloc_slot(&mut self, generation: u64) -> Option<(usize, u32)> {
        // First pass: find an unallocated slot.
        if let Some(idx) = self.slots.iter().position(|s| !s.allocated) {
            self.slots[idx] = SlotState { generation, allocated: true };
            return Some((idx, (idx * self.slot_size) as u32));
        }
        // Second pass: reuse the oldest (lowest generation) slot.
        let oldest = self.slots.iter().enumerate().min_by_key(|(_, s)| s.generation)?;
        let idx = oldest.0;
        self.slots[idx] = SlotState { generation, allocated: true };
        Some((idx, (idx * self.slot_size) as u32))
    }

    /// Mark a slot as ready (allocated but no longer pending render).
    pub fn mark_ready(&mut self, slot_index: usize) {
        if let Some(slot) = self.slots.get_mut(slot_index) {
            slot.allocated = false;
        }
    }

    /// Invalidate all slots with generation <= the given value. [SDS §5.3]
    pub fn invalidate_up_to(&mut self, generation: u64) {
        for slot in &mut self.slots {
            if slot.generation <= generation && slot.generation != 0 {
                slot.allocated = false;
            }
        }
    }

    /// Read-only access to a slot's bytes.
    pub fn slot_bytes(&self, slot_index: usize) -> &[u8] {
        let start = slot_index * self.slot_size;
        &self.region.as_slice()[start..start + self.slot_size]
    }

    /// Mutable access to a slot's bytes (for the worker side).
    pub fn slot_bytes_mut(&mut self, slot_index: usize) -> &mut [u8] {
        let start = slot_index * self.slot_size;
        &mut self.region.as_mut_slice()[start..start + self.slot_size]
    }

    /// Flush the shared region to ensure visibility.
    pub fn flush(&self) -> Result<(), String> {
        self.region.flush().map_err(|e| e.to_string())
    }
}

/// Map a shared-memory file as a tile pool (worker side).
///
/// Returns the mmap'd slice and the number of slots it can hold.
pub fn map_pool(file: &File, total_bytes: usize) -> Result<memmap2::MmapMut, String> {
    map_shmem_file(file, total_bytes).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pool_create_and_alloc() {
        let mut pool = TilePool::create(4).unwrap();
        assert_eq!(pool.slot_count(), 4);
        assert_eq!(pool.len(), 4 * TILE_RGBA8_BYTES);

        let (idx0, off0) = pool.alloc_slot(1).unwrap();
        assert_eq!(idx0, 0);
        assert_eq!(off0, 0);

        let (idx1, off1) = pool.alloc_slot(2).unwrap();
        assert_eq!(idx1, 1);
        assert_eq!(off1, TILE_RGBA8_BYTES as u32);

        // Mark slot 0 ready, should be reusable.
        pool.mark_ready(idx0);
        let (idx2, _) = pool.alloc_slot(3).unwrap();
        assert_eq!(idx2, 0); // reused
    }

    #[test]
    fn pool_invalidate() {
        let mut pool = TilePool::create(2).unwrap();
        pool.alloc_slot(5);
        pool.alloc_slot(6);
        pool.invalidate_up_to(5);
        // Slot with gen 5 should be freed; gen 6 should remain.
        // alloc should reuse slot 0 (gen 5, now free).
        let (idx, _) = pool.alloc_slot(7).unwrap();
        assert_eq!(idx, 0);
    }

    #[test]
    fn pool_slot_bytes_read_write() {
        let mut pool = TilePool::create(1).unwrap();
        let (idx, _) = pool.alloc_slot(1).unwrap();
        pool.slot_bytes_mut(idx)[0] = 0xAB;
        pool.flush().unwrap();
        assert_eq!(pool.slot_bytes(idx)[0], 0xAB);
    }
}
