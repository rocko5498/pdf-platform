//! Engine capability traits. No backend code here; only contracts. [ADR-005]
//! Invariant: engine-api MUST NOT know any backend. [ADR-025]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod extract; // ExtractCapability: text-with-geometry per page
pub mod rasterize; // RasterizeCapability: render page region → shmem tile
pub mod structure; // StructureCapability: enumerate structure tree / forms
