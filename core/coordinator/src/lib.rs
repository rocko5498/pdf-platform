//! Coordinator: trusted brain of the UI process. [ADR-010, SDS §2.2]
//! Single-writer invariant: only DocumentCoordinator mutates a document's overlay. [SDS §1.4]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod document; // DocumentCoordinator actor; per-document owned state + channel inbox
pub mod render;   // RenderScheduler: viewport → tile requests [SDS §2.2.2]
pub mod memory;   // CacheGovernor + MemoryGovernor [SDS §2.2.4, ADR-011]
pub mod broker;   // Broker: sole executor of privileged ops [SDS §2.2.6, ADR-016]
pub mod session;  // session lifecycle: worker spawn/respawn/crash-recovery [SDS §10.1]
pub mod settings; // Settings: UI prefs + enterprise policy overlay [SDS §2.2.11]
pub mod inspect;  // Synchronous inspect for CLI diagnostic path [ADR-010, FR-DIAG-2]
