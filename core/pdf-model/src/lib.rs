//! Semantic faades, Commands, UndoJournal. [ADR-006, ADR-013, SDS §2.2]
//!
//! The model layer sits above the COS store and provides:
//! - **CoW overlay**: copy-on-write layer over the original document bytes.
//!   Every mutation creates new object versions in the overlay; original
//!   bytes are immutable ground truth. [ADR-006]
//! - **Commands**: named, parameterized mutations that produce forward deltas
//!   and own their inversion information. [ADR-013]
//! - **UndoJournal**: append-only log of command groups, supporting unlimited
//!   undo/redo with crash recovery via sidecar persistence. [ADR-013, ADR-021]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod annotation;
pub mod appearance;
pub mod page_patch;
pub mod assembly;
pub mod assembly_ops;
pub mod command;
pub mod fdf;
pub mod form;
pub mod forms_js;
pub mod journal;
pub mod overlay;
pub mod organize;
pub mod redaction;
pub mod review;
