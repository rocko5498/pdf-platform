//! Tile LRU cache with byte-weighted eviction. [ADR-007, SDS §8.1]
//!
//! The cache holds rendered tile descriptors keyed by (page, col, row, generation).
//! Under memory pressure the least-recently-used tiles are evicted first.
//! Invalidation is generation-keyed: an edit bumps the generation and only
//! stale-revision tiles of changed pages are evicted. [SDS §8.1]

use std::collections::{HashMap, VecDeque};

use protocol::handles::TileSlotDesc;

/// Key for a cached tile. Uniquely identifies a tile across revisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TileCacheKey {
    /// 0-based page index.
    pub page: u32,
    /// Column in the tile grid.
    pub col: u32,
    /// Row in the tile grid.
    pub row: u32,
}

/// A cached tile entry with its descriptor and generation.
#[derive(Debug, Clone)]
pub struct CacheEntry {
    /// Slot descriptor for reading pixels from shmem.
    pub desc: TileSlotDesc,
    /// Generation when this tile was rendered.
    pub generation: u64,
}

/// Byte-weighted LRU tile cache. [GR-7: bounded, eviction policy]
///
/// Tracks total cached bytes and evicts least-recently-used entries when
/// the budget is exceeded. Generation-keyed invalidation ensures edits
/// only discard stale tiles.
pub struct TileCache {
    entries: HashMap<TileCacheKey, CacheEntry>,
    /// LRU order: front = least recently used, back = most recently used.
    lru: VecDeque<TileCacheKey>,
    /// Maximum bytes before eviction begins.
    max_bytes: usize,
    /// Current bytes used by cached tiles.
    current_bytes: usize,
    /// Bytes per tile slot (for accounting).
    tile_bytes: usize,
}

impl std::fmt::Debug for TileCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TileCache")
            .field("entries", &self.entries.len())
            .field("current_bytes", &self.current_bytes)
            .field("max_bytes", &self.max_bytes)
            .field("tile_bytes", &self.tile_bytes)
            .finish()
    }
}

impl TileCache {
    /// Create a new cache with the given byte budget.
    ///
    /// `tile_bytes` is the size of one tile slot (typically `TILE_RGBA8_BYTES`).
    /// `max_bytes` is the total budget; entries beyond this trigger LRU eviction.
    pub fn new(max_bytes: usize, tile_bytes: usize) -> Self {
        Self {
            entries: HashMap::new(),
            lru: VecDeque::new(),
            max_bytes,
            current_bytes: 0,
            tile_bytes,
        }
    }

    /// Look up a cached tile. Promotes it to most-recently-used on hit.
    pub fn get(&mut self, key: &TileCacheKey) -> Option<&CacheEntry> {
        if self.entries.contains_key(key) {
            self.promote(key);
            self.entries.get(key)
        } else {
            None
        }
    }

    /// Insert a tile into the cache. Evicts LRU entries if over budget.
    ///
    /// If a tile with the same key already exists, it is replaced.
    pub fn insert(&mut self, key: TileCacheKey, entry: CacheEntry) {
        // Remove existing entry if present.
        if self.entries.remove(&key).is_some() {
            self.current_bytes = self.current_bytes.saturating_sub(self.tile_bytes);
            self.lru.retain(|k| *k != key);
        }

        // Evict until we have room.
        while self.current_bytes + self.tile_bytes > self.max_bytes && !self.lru.is_empty() {
            self.evict_lru();
        }

        // Insert.
        self.entries.insert(key, entry);
        self.lru.push_back(key);
        self.current_bytes += self.tile_bytes;
    }

    /// Remove a specific tile from the cache.
    pub fn remove(&mut self, key: &TileCacheKey) -> Option<CacheEntry> {
        if let Some(entry) = self.entries.remove(key) {
            self.current_bytes = self.current_bytes.saturating_sub(self.tile_bytes);
            self.lru.retain(|k| k != key);
            Some(entry)
        } else {
            None
        }
    }

    /// Invalidate all tiles with generation <= the given value. [SDS §8.1]
    ///
    /// Returns the number of tiles evicted.
    pub fn invalidate_up_to(&mut self, generation: u64) -> usize {
        let stale: Vec<TileCacheKey> = self
            .entries
            .iter()
            .filter(|(_, e)| e.generation <= generation && e.generation != 0)
            .map(|(k, _)| *k)
            .collect();
        let count = stale.len();
        for key in stale {
            self.remove(&key);
        }
        count
    }

    /// Invalidate all tiles for a specific page with generation <= the given value.
    ///
    /// This is the targeted invalidation path used after an edit: only the
    /// changed page's tiles are evicted. [SDS §6.6, §8.1]
    pub fn invalidate_page(&mut self, page: u32, generation: u64) -> usize {
        let stale: Vec<TileCacheKey> = self
            .entries
            .iter()
            .filter(|(k, e)| k.page == page && e.generation <= generation && e.generation != 0)
            .map(|(k, _)| *k)
            .collect();
        let count = stale.len();
        for key in stale {
            self.remove(&key);
        }
        count
    }

    /// Clear all entries.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.lru.clear();
        self.current_bytes = 0;
    }

    /// Number of cached tiles.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Total bytes used by cached tiles.
    pub fn current_bytes(&self) -> usize {
        self.current_bytes
    }

    /// Maximum byte budget.
    pub fn max_bytes(&self) -> usize {
        self.max_bytes
    }

    /// Promote a key to most-recently-used.
    fn promote(&mut self, key: &TileCacheKey) {
        self.lru.retain(|k| k != key);
        self.lru.push_back(*key);
    }

    /// Evict the least-recently-used entry.
    fn evict_lru(&mut self) {
        if let Some(key) = self.lru.pop_front() {
            self.entries.remove(&key);
            self.current_bytes = self.current_bytes.saturating_sub(self.tile_bytes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::handles::{PixelFormat, TILE_RGBA8_BYTES};

    fn make_entry(gen: u64) -> CacheEntry {
        CacheEntry {
            desc: TileSlotDesc {
                offset: 0,
                len: TILE_RGBA8_BYTES as u32,
                format: PixelFormat::Rgba8,
                generation: gen,
                page: 0,
                col: 0,
                row: 0,
            },
            generation: gen,
        }
    }

    #[test]
    fn cache_insert_and_get() {
        let mut cache = TileCache::new(1024 * 1024, TILE_RGBA8_BYTES);
        let key = TileCacheKey { page: 0, col: 0, row: 0 };
        cache.insert(key, make_entry(1));
        assert!(cache.get(&key).is_some());
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.current_bytes(), TILE_RGBA8_BYTES);
    }

    #[test]
    fn cache_evicts_lru_when_full() {
        // Budget = 2 tiles.
        let mut cache = TileCache::new(TILE_RGBA8_BYTES * 2, TILE_RGBA8_BYTES);

        let k0 = TileCacheKey { page: 0, col: 0, row: 0 };
        let k1 = TileCacheKey { page: 0, col: 1, row: 0 };
        let k2 = TileCacheKey { page: 0, col: 2, row: 0 };

        cache.insert(k0, make_entry(1));
        cache.insert(k1, make_entry(2));

        // k0 is LRU; inserting k2 should evict k0.
        cache.insert(k2, make_entry(3));

        assert!(cache.get(&k0).is_none(), "k0 should be evicted");
        assert!(cache.get(&k1).is_some());
        assert!(cache.get(&k2).is_some());
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn cache_get_promotes_to_mru() {
        let mut cache = TileCache::new(TILE_RGBA8_BYTES * 2, TILE_RGBA8_BYTES);

        let k0 = TileCacheKey { page: 0, col: 0, row: 0 };
        let k1 = TileCacheKey { page: 0, col: 1, row: 0 };
        let k2 = TileCacheKey { page: 0, col: 2, row: 0 };

        cache.insert(k0, make_entry(1));
        cache.insert(k1, make_entry(2));

        // Access k0 to promote it.
        cache.get(&k0);

        // Now k1 is LRU; inserting k2 should evict k1.
        cache.insert(k2, make_entry(3));

        assert!(cache.get(&k0).is_some(), "k0 should survive (was promoted)");
        assert!(cache.get(&k1).is_none(), "k1 should be evicted (was LRU)");
        assert!(cache.get(&k2).is_some());
    }

    #[test]
    fn cache_invalidate_up_to() {
        let mut cache = TileCache::new(1024 * 1024, TILE_RGBA8_BYTES);
        let k0 = TileCacheKey { page: 0, col: 0, row: 0 };
        let k1 = TileCacheKey { page: 0, col: 1, row: 0 };
        cache.insert(k0, make_entry(5));
        cache.insert(k1, make_entry(6));

        let evicted = cache.invalidate_up_to(5);
        assert_eq!(evicted, 1);
        assert!(cache.get(&k0).is_none());
        assert!(cache.get(&k1).is_some());
    }

    #[test]
    fn cache_invalidate_page() {
        let mut cache = TileCache::new(1024 * 1024, TILE_RGBA8_BYTES);
        let k0 = TileCacheKey { page: 0, col: 0, row: 0 };
        let k1 = TileCacheKey { page: 1, col: 0, row: 0 };
        cache.insert(k0, make_entry(5));
        cache.insert(k1, make_entry(5));

        let evicted = cache.invalidate_page(0, 5);
        assert_eq!(evicted, 1);
        assert!(cache.get(&k0).is_none(), "page 0 should be evicted");
        assert!(cache.get(&k1).is_some(), "page 1 should survive");
    }

    #[test]
    fn cache_clear() {
        let mut cache = TileCache::new(1024 * 1024, TILE_RGBA8_BYTES);
        let k0 = TileCacheKey { page: 0, col: 0, row: 0 };
        cache.insert(k0, make_entry(1));
        assert_eq!(cache.len(), 1);
        cache.clear();
        assert_eq!(cache.len(), 0);
        assert_eq!(cache.current_bytes(), 0);
    }

    #[test]
    fn cache_replace_existing_key() {
        let mut cache = TileCache::new(1024 * 1024, TILE_RGBA8_BYTES);
        let key = TileCacheKey { page: 0, col: 0, row: 0 };
        cache.insert(key, make_entry(1));
        cache.insert(key, make_entry(2));
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.get(&key).unwrap().generation, 2);
    }
}
