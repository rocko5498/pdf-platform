//! Memory-pressure callbacks; coordinates shmem pool eviction. [SDS §9]

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

/// Invalid global/cache memory budget configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryGovernorError {
    /// A budget is zero or exceeds its parent ceiling.
    InvalidBudget,
}

#[derive(Debug)]
struct MemoryLedger {
    global_ceiling: usize,
    thumbnail_budget: usize,
    thumbnail_usage: usize,
}

/// Shared byte-accounting authority for reconstructible coordinator caches.
#[derive(Debug, Clone)]
pub struct MemoryGovernor {
    ledger: Arc<Mutex<MemoryLedger>>,
}

impl MemoryGovernor {
    /// Create a governor with a global ceiling and thumbnail sub-budget.
    pub fn new(
        global_ceiling: usize,
        thumbnail_budget: usize,
    ) -> Result<Self, MemoryGovernorError> {
        if global_ceiling == 0 || thumbnail_budget == 0 || thumbnail_budget > global_ceiling {
            return Err(MemoryGovernorError::InvalidBudget);
        }
        Ok(Self {
            ledger: Arc::new(Mutex::new(MemoryLedger {
                global_ceiling,
                thumbnail_budget,
                thumbnail_usage: 0,
            })),
        })
    }

    /// Current thumbnail-cache bytes for diagnostics.
    pub fn thumbnail_usage(&self) -> usize {
        self.ledger
            .lock()
            .map(|ledger| ledger.thumbnail_usage)
            .unwrap_or(0)
    }

    fn reserve_thumbnail(&self, bytes: usize) -> bool {
        let Ok(mut ledger) = self.ledger.lock() else {
            return false;
        };
        let Some(next) = ledger.thumbnail_usage.checked_add(bytes) else {
            return false;
        };
        if next > ledger.thumbnail_budget || next > ledger.global_ceiling {
            return false;
        }
        ledger.thumbnail_usage = next;
        true
    }

    fn release_thumbnail(&self, bytes: usize) {
        if let Ok(mut ledger) = self.ledger.lock() {
            ledger.thumbnail_usage = ledger.thumbnail_usage.saturating_sub(bytes);
        }
    }
}

/// Revision- and generation-keyed thumbnail cache identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ThumbnailCacheKey {
    /// Coordinator document identity.
    pub document: u64,
    /// Zero-based page index.
    pub page: u32,
    /// Pixel width.
    pub width: u32,
    /// Pixel height.
    pub height: u32,
    /// Render generation.
    pub generation: u64,
    /// Document revision.
    pub revision: u64,
}

/// Rejected thumbnail cache insertion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThumbnailCacheError {
    /// Pixel bytes do not exactly match `width × height × 4`.
    InvalidPixelLength,
    /// One entry cannot fit even after shedding all cached thumbnails.
    BudgetExceeded,
}

/// Exact-byte, least-recently-used thumbnail cache governed by a shared ceiling.
pub struct ThumbnailCache {
    governor: MemoryGovernor,
    entries: HashMap<ThumbnailCacheKey, Vec<u8>>,
    lru: VecDeque<ThumbnailCacheKey>,
}

impl ThumbnailCache {
    /// Create an empty cache governed by `governor`.
    pub fn new(governor: MemoryGovernor) -> Self {
        Self {
            governor,
            entries: HashMap::new(),
            lru: VecDeque::new(),
        }
    }

    /// Insert pixels, evicting least-recently-used thumbnails until they fit.
    pub fn insert(
        &mut self,
        key: ThumbnailCacheKey,
        pixels: Vec<u8>,
    ) -> Result<(), ThumbnailCacheError> {
        let expected = key
            .width
            .checked_mul(key.height)
            .and_then(|value| value.checked_mul(4))
            .and_then(|value| usize::try_from(value).ok())
            .ok_or(ThumbnailCacheError::InvalidPixelLength)?;
        if pixels.len() != expected {
            return Err(ThumbnailCacheError::InvalidPixelLength);
        }
        self.remove(&key);
        while !self.governor.reserve_thumbnail(pixels.len()) {
            if !self.evict_lru() {
                return Err(ThumbnailCacheError::BudgetExceeded);
            }
        }
        self.entries.insert(key, pixels);
        self.lru.push_back(key);
        Ok(())
    }

    /// Read pixels and promote the entry to most-recently-used.
    pub fn get(&mut self, key: &ThumbnailCacheKey) -> Option<&[u8]> {
        if !self.entries.contains_key(key) {
            return None;
        }
        self.lru.retain(|candidate| candidate != key);
        self.lru.push_back(*key);
        self.entries.get(key).map(Vec::as_slice)
    }

    /// Remove entries older than `generation`, returning the eviction count.
    pub fn invalidate_before_generation(&mut self, generation: u64) -> usize {
        let stale: Vec<_> = self
            .entries
            .keys()
            .filter(|key| key.generation < generation)
            .copied()
            .collect();
        for key in &stale {
            self.remove(key);
        }
        stale.len()
    }

    fn remove(&mut self, key: &ThumbnailCacheKey) {
        if let Some(pixels) = self.entries.remove(key) {
            self.governor.release_thumbnail(pixels.len());
            self.lru.retain(|candidate| candidate != key);
        }
    }

    fn evict_lru(&mut self) -> bool {
        let Some(key) = self.lru.pop_front() else {
            return false;
        };
        if let Some(pixels) = self.entries.remove(&key) {
            self.governor.release_thumbnail(pixels.len());
        }
        true
    }
}

impl Drop for ThumbnailCache {
    fn drop(&mut self) {
        let bytes = self.entries.values().map(Vec::len).sum();
        self.governor.release_thumbnail(bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(page: u32, generation: u64) -> ThumbnailCacheKey {
        ThumbnailCacheKey {
            document: 1,
            page,
            width: 2,
            height: 2,
            generation,
            revision: 3,
        }
    }

    #[test]
    fn thumbnail_cache_evicts_lru_under_governed_budget() {
        let governor = MemoryGovernor::new(32, 32).unwrap();
        let mut cache = ThumbnailCache::new(governor.clone());
        cache.insert(key(0, 1), vec![1; 16]).unwrap();
        cache.insert(key(1, 1), vec![2; 16]).unwrap();
        assert!(cache.get(&key(0, 1)).is_some());

        cache.insert(key(2, 1), vec![3; 16]).unwrap();

        assert!(cache.get(&key(0, 1)).is_some());
        assert!(cache.get(&key(1, 1)).is_none());
        assert!(cache.get(&key(2, 1)).is_some());
        assert_eq!(governor.thumbnail_usage(), 32);
    }

    #[test]
    fn thumbnail_cache_discards_stale_generation_and_releases_memory() {
        let governor = MemoryGovernor::new(64, 64).unwrap();
        let mut cache = ThumbnailCache::new(governor.clone());
        cache.insert(key(0, 1), vec![1; 16]).unwrap();
        cache.insert(key(1, 2), vec![2; 16]).unwrap();

        assert_eq!(cache.invalidate_before_generation(2), 1);
        assert_eq!(governor.thumbnail_usage(), 16);
    }
}
