//! Sole Rust↔Qt FFI boundary. [ADR-004]
//! RULES (enforced in review):
//!   FFI-1: cxx-checked interface only — no hand-rolled ABI.
//!   FFI-3: no raw pointers owned across the boundary.
//!   FFI-4: carries commands/events/handles only — never document objects.
//!   FFI-6: two-reviewer rule; changes require one FFI-surface owner. [ADR-027]
// SAFETY: cxx guarantees type-checked cross-language calls; no exceptions cross
//         this boundary; ownership does not straddle languages. [ADR-004, ADR-027]

#[cxx::bridge(namespace = "pdf_platform")]
mod ffi {
    // Commands: shell → coordinator (submitted async; bridge returns immediately)
    // Events:   coordinator → shell  (marshalled onto Qt main thread via queued dispatcher)
    // Handles:  shmem tile descriptors for GPU upload (shell maps, never copies)
    // ponytail: stub — define command/event types at M0 implementation
}
