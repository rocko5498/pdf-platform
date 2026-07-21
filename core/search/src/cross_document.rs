//! Bounded cross-document index staging from the canonical text model. [ADR-019]

use std::collections::HashMap;

use engine_api::extract::PageTextModel;

/// One path-free, revision-keyed page ready for the index backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexRecord {
    /// Opaque Z0-assigned source identity; never a filesystem path.
    pub source: [u8; 16],
    /// Source file revision identity.
    pub revision: u64,
    /// Zero-based page index.
    pub page: u32,
    /// Canonical text joined in reading order.
    pub text: String,
    /// Whether extraction was trustworthy enough for normal search claims.
    pub reliable: bool,
}

/// Invalid bounded staging operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexStagingError {
    /// Staging must declare a non-zero byte ceiling.
    ZeroBudget,
    /// New canonical text would exceed the declared ceiling.
    BudgetExceeded,
}

/// Exact-byte bounded staging area in front of the reviewed index backend.
pub struct IndexStaging {
    records: HashMap<([u8; 16], u32), IndexRecord>,
    max_bytes: usize,
    current_bytes: usize,
}

impl IndexStaging {
    /// Create empty staging with an exact UTF-8 text-byte ceiling.
    pub fn new(max_bytes: usize) -> Result<Self, IndexStagingError> {
        if max_bytes == 0 {
            return Err(IndexStagingError::ZeroBudget);
        }
        Ok(Self {
            records: HashMap::new(),
            max_bytes,
            current_bytes: 0,
        })
    }

    /// Ingest one canonical page, replacing an older revision atomically.
    pub fn ingest(
        &mut self,
        source: [u8; 16],
        revision: u64,
        model: &PageTextModel,
    ) -> Result<(), IndexStagingError> {
        let text = model
            .lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let key = (source, model.page_index);
        let old_bytes = self.records.get(&key).map_or(0, |record| record.text.len());
        let next_bytes = self
            .current_bytes
            .saturating_sub(old_bytes)
            .checked_add(text.len())
            .ok_or(IndexStagingError::BudgetExceeded)?;
        if next_bytes > self.max_bytes {
            return Err(IndexStagingError::BudgetExceeded);
        }
        self.records.insert(
            key,
            IndexRecord {
                source,
                revision,
                page: model.page_index,
                text,
                reliable: model.reliable,
            },
        );
        self.current_bytes = next_bytes;
        Ok(())
    }

    /// Number of staged pages.
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Whether no pages are staged.
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Iterate staged records for the contained backend seam.
    pub fn records(&self) -> impl Iterator<Item = &IndexRecord> {
        self.records.values()
    }

    /// Current exact canonical-text byte usage.
    pub fn current_bytes(&self) -> usize {
        self.current_bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_api::extract::{PageTextModel, TextLine};

    fn model(page: u32, text: &str, reliable: bool) -> PageTextModel {
        PageTextModel {
            page_index: page,
            lines: vec![TextLine {
                index: 0,
                text: text.into(),
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
                spans: Vec::new(),
            }],
            reliable,
            char_count: text.chars().count() as u32,
            has_structure: false,
        }
    }

    #[test]
    fn ingest_replaces_changed_revision_and_preserves_unreliable_flag() {
        let source = [4; 16];
        let mut staging = IndexStaging::new(1024).unwrap();
        staging.ingest(source, 1, &model(0, "old", true)).unwrap();
        staging.ingest(source, 2, &model(0, "new", false)).unwrap();

        assert_eq!(staging.len(), 1);
        let record = staging.records().next().unwrap();
        assert_eq!(record.revision, 2);
        assert_eq!(record.text, "new");
        assert!(!record.reliable);
    }

    #[test]
    fn ingest_rejects_text_beyond_declared_budget() {
        let mut staging = IndexStaging::new(4).unwrap();
        assert_eq!(
            staging.ingest([1; 16], 1, &model(0, "12345", true)),
            Err(IndexStagingError::BudgetExceeded)
        );
    }
}
