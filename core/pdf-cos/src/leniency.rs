//! Tolerated deviations from the PDF specification. [ADR-006, FR-DIAG-1]

/// A single tolerated parse deviation recorded during document scanning.
#[derive(Debug, Clone)]
pub struct LeniencyEvent {
    pub kind: &'static str,
    pub detail: String,
}

impl LeniencyEvent {
    pub fn new(kind: &'static str, detail: impl Into<String>) -> Self {
        Self { kind, detail: detail.into() }
    }
}

impl std::fmt::Display for LeniencyEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.kind, self.detail)
    }
}
