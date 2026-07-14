//! Synchronous document inspect command. [ADR-010, ADR-025, FR-DIAG-2]
//!
//! The CLI links coordinator in-process; this path is synchronous and does not
//! use the actor/channel model (which applies to the document open/render lifecycle).

use pdf_cos::scan::{scan_structure, ScanError};
use protocol::inspect::StructuralSummary;
use std::path::Path;

/// Error returned by [`inspect`].
#[derive(Debug)]
pub enum InspectError {
    /// Document scan failed.
    Scan(ScanError),
}

impl std::fmt::Display for InspectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InspectError::Scan(e) => write!(f, "scan failed: {e}"),
        }
    }
}

impl std::error::Error for InspectError {}

/// Inspect a PDF file and return its structural summary.
///
/// Synchronous — no worker spawn, no shmem, no sandbox for this path.
pub fn inspect(path: &Path) -> Result<StructuralSummary, InspectError> {
    let ds = scan_structure(path).map_err(InspectError::Scan)?;
    // ponytail: exhaustive field map — when DocumentStructure gains fields,
    // update StructuralSummary in protocol::inspect and this mapping together.
    Ok(StructuralSummary {
        page_count: ds.page_count,
        has_acroform: ds.has_acroform,
        has_xfa: ds.has_xfa,
        has_js: ds.has_js,
        sig_count: ds.sig_count,
        leniency_count: ds.leniency.len() as u32,
        leniency_events: ds.leniency.iter().map(|e| e.to_string()).collect(),
    })
}
