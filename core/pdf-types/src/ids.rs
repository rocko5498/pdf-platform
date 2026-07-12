//! Typed document identifiers. [SDS §3, ADR-025]
// ponytail: stub — expand at M1

/// Opaque document handle (coordinator-assigned, never crosses trust zones).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DocumentId(pub u64);

/// Zero-indexed page number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PageIndex(pub u32);
