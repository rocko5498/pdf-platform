//! Tile scheduler, generation counters, priority queues, shmem tile transport. [ADR-007]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod cache; // LRU tile cache with byte-weighted eviction [SDS §8.1]
pub mod scheduler; // RenderScheduler: decompose viewport → tile requests, priority ordering
pub mod shmem; // shared-memory buffer pool; handle lifecycle [ADR-031]
