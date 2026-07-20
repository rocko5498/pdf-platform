//! Coordinator: trusted brain of the UI process. [ADR-010, SDS §2.2]
//! Single-writer invariant: only DocumentCoordinator mutates a document's overlay. [SDS §1.4]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod broker; // Broker: sole executor of privileged ops [SDS §2.2.6, ADR-016]
pub mod document; // DocumentCoordinator actor; per-document owned state + channel inbox
pub mod inspect; // Synchronous inspect for CLI diagnostic path [ADR-010, FR-DIAG-2]
pub mod memory; // CacheGovernor + MemoryGovernor [SDS §2.2.4, ADR-011]
pub mod plugin; // CoordinatorPluginHost: Z0 plugin control plane [SDS §2.2.7, ADR-014]
pub mod render; // RenderScheduler: viewport → tile requests [SDS §2.2.2]
pub mod session; // WorkerSession: spawn/poll/death/inspect/respawn [SDS §10.1, §3.1]
pub mod settings; // Settings: UI prefs + enterprise policy overlay [SDS §2.2.11]
