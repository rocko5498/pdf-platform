//! Inspect command result type. [ADR-025, FR-DIAG-2]

/// Structural summary of a PDF document, returned by the inspect command.
/// Wire type owned by protocol; pdf-cos owns the raw parse result.
#[derive(Debug, Clone)]
pub struct StructuralSummary {
    pub page_count: u32,
    pub has_acroform: bool,
    pub has_xfa: bool,
    pub has_js: bool,
    pub sig_count: u32,
    pub leniency_count: u32,
    /// Human-readable leniency event descriptions (M0: strings; M6: structured).
    pub leniency_events: Vec<String>,
}
