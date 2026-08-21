//! Typed command / event / handle-descriptor protocol. [ADR-004, ADR-031]
//! Invariant: nothing in this crate may reference Qt types. [ADR-025]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod commands; // Commands: OpenDocument, Navigate, RequestTiles, ApplyAnnotation, …
pub mod events; // Events: DocumentOpened, TilesReady, DocumentChanged, WorkerDied, …
pub mod handles; // ShmemHandle, TileDescriptor, CorrelationId
pub mod inspect; // StructuralSummary: wire type for inspect command result
pub mod transport; // WorkerTransport trait (ADR-031); platform impls live in sandbox/
pub mod utility_indexing; // Path-free canonical-text index requests. [ADR-019]
pub mod utility_jobs; // Declarative utility job command/result frames. [ADR-009]
pub mod utility_thumbnails; // Bounded utility thumbnail metadata. [ADR-007, ADR-009]
