//! Tile scheduler, generation counters, priority queues, shmem tile transport. [ADR-007]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod scheduler; // RenderScheduler: decompose viewport → tile requests, priority ordering
pub mod shmem;     // shared-memory buffer pool; handle lifecycle [ADR-031]
pub mod cache;     // tile-cache key (revision, page, scale-bucket, coord, rotation) [SDS §8.1]
