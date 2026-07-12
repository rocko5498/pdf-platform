//! hayro experimental rasterizer stub. Compile-verified against engine-api traits. [ADR-005]
//! Provides differential rendering data; promoted to default rasterizer per ADR-005 criteria.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

// ponytail: stub — type-checks against engine-api traits; no implementation yet
pub mod backend; // HayroEngine placeholder; will impl RasterizeCapability
