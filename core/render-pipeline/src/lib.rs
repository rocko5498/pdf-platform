//! Tile scheduler, generation counters, priority queues, shmem tile transport. [ADR-007]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod cache; // LRU tile cache with byte-weighted eviction [SDS §8.1]
pub mod input; // Tool modes, keyboard shortcuts, mouse drag, view controller
pub mod layout; // Grid layout, page positioning, viewport state, scale bucketing
pub mod scheduler; // RenderScheduler: decompose viewport → tile requests, priority ordering
pub mod scroll; // Scroll physics: velocity tracking, momentum, edge resistance [SDS §6.8]
pub mod shmem; // shared-memory buffer pool; handle lifecycle [ADR-031]
pub mod thumbnail; // Engine-neutral bounded thumbnail rendering. [ADR-005, ADR-009]
