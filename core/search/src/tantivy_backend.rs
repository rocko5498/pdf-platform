//! Tantivy-backed cross-document index. [ADR-019 §3, ADR-034]
//!
//! Consumes [`IndexRecord`]s already bounded/staged by
//! [`crate::cross_document::IndexStaging`] and writes them into a local
//! Tantivy index. Index *writes* are local file I/O against a per-user
//! app-state directory (never a document-derived path) — a Z0/broker-owned
//! operation per ADR-016's privileged-file-write principle, not something
//! that needs to run inside a sandboxed utility worker. `IndexRecord` is
//! already path-free and revision-keyed by the time it reaches here.

use std::path::Path;

use tantivy::directory::MmapDirectory;
use tantivy::query::QueryParser;
use tantivy::schema::{Field, Schema, Value, STORED, STRING, TEXT};
use tantivy::{collector::TopDocs, doc, Index, IndexWriter, TantivyDocument, Term};

use crate::cross_document::IndexRecord;

/// One matched page from a cross-document search. [FR-SRCH-IDX]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    /// Opaque source identity (matches [`IndexRecord::source`]).
    pub source: [u8; 16],
    /// Zero-based page index.
    pub page: u32,
    /// Whether the source text was extraction-reliable when indexed.
    pub reliable: bool,
    /// Relevance score, truncated to an integer milli-score for `Eq`.
    pub score_milli: i64,
}

/// Failure opening, writing to, or querying the index.
#[derive(Debug)]
pub enum TantivyBackendError {
    /// Underlying Tantivy error (I/O, corruption, directory open failure).
    Backend(String),
    /// Query syntax was invalid.
    Query(String),
}

impl std::fmt::Display for TantivyBackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Backend(m) => write!(f, "tantivy backend error: {m}"),
            Self::Query(m) => write!(f, "invalid query: {m}"),
        }
    }
}

impl std::error::Error for TantivyBackendError {}

fn build_schema() -> Schema {
    let mut builder = Schema::builder();
    builder.add_text_field("key", STRING | STORED);
    builder.add_text_field("source", STRING | STORED);
    builder.add_u64_field("revision", STORED);
    builder.add_u64_field("page", STORED);
    builder.add_u64_field("reliable", STORED);
    builder.add_text_field("text", TEXT | STORED);
    builder.build()
}

fn source_hex(source: [u8; 16]) -> String {
    source.iter().map(|b| format!("{b:02x}")).collect()
}

fn decode_source_hex(hex: &str) -> [u8; 16] {
    let mut out = [0u8; 16];
    for (i, chunk) in hex.as_bytes().chunks(2).enumerate().take(16) {
        if let Ok(s) = std::str::from_utf8(chunk) {
            if let Ok(byte) = u8::from_str_radix(s, 16) {
                out[i] = byte;
            }
        }
    }
    out
}

fn record_key(source: [u8; 16], page: u32) -> String {
    format!("{}:{page}", source_hex(source))
}

/// Local cross-document Tantivy index. [ADR-019 §3, SDS §2.2.9]
pub struct CrossDocumentIndex {
    index: Index,
    writer: IndexWriter,
    key_field: Field,
    source_field: Field,
    revision_field: Field,
    page_field: Field,
    reliable_field: Field,
    text_field: Field,
}

impl CrossDocumentIndex {
    /// Open the index at `dir`, creating it if absent.
    pub fn open_or_create(dir: &Path) -> Result<Self, TantivyBackendError> {
        std::fs::create_dir_all(dir).map_err(|e| TantivyBackendError::Backend(e.to_string()))?;
        let schema = build_schema();
        let directory =
            MmapDirectory::open(dir).map_err(|e| TantivyBackendError::Backend(e.to_string()))?;
        let index = Index::builder()
            .schema(schema.clone())
            .open_or_create(directory)
            .map_err(|e| TantivyBackendError::Backend(e.to_string()))?;
        let writer: IndexWriter = index
            .writer(50_000_000)
            .map_err(|e| TantivyBackendError::Backend(e.to_string()))?;
        Ok(Self {
            key_field: schema.get_field("key").expect("schema defines key"),
            source_field: schema.get_field("source").expect("schema defines source"),
            revision_field: schema
                .get_field("revision")
                .expect("schema defines revision"),
            page_field: schema.get_field("page").expect("schema defines page"),
            reliable_field: schema
                .get_field("reliable")
                .expect("schema defines reliable"),
            text_field: schema.get_field("text").expect("schema defines text"),
            index,
            writer,
        })
    }

    /// Upsert one staged record: replaces any prior revision for the same
    /// `(source, page)` atomically at commit. [ADR-019 §3]
    pub fn upsert(&mut self, record: &IndexRecord) -> Result<(), TantivyBackendError> {
        let key = record_key(record.source, record.page);
        self.writer
            .delete_term(Term::from_field_text(self.key_field, &key));
        self.writer
            .add_document(doc!(
                self.key_field => key,
                self.source_field => source_hex(record.source),
                self.revision_field => record.revision,
                self.page_field => u64::from(record.page),
                self.reliable_field => u64::from(record.reliable),
                self.text_field => record.text.clone(),
            ))
            .map_err(|e| TantivyBackendError::Backend(e.to_string()))?;
        self.writer
            .commit()
            .map_err(|e| TantivyBackendError::Backend(e.to_string()))?;
        Ok(())
    }

    /// Remove every page indexed for `source` — used when an enrollment is
    /// removed or a document is deleted (settings visibility, ADR-019 §3).
    pub fn remove_source(&mut self, source: [u8; 16]) -> Result<(), TantivyBackendError> {
        self.writer
            .delete_term(Term::from_field_text(self.source_field, &source_hex(source)));
        self.writer
            .commit()
            .map_err(|e| TantivyBackendError::Backend(e.to_string()))?;
        Ok(())
    }

    /// Full-text query across all indexed pages. [FR-SRCH-IDX]
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>, TantivyBackendError> {
        let reader = self
            .index
            .reader()
            .map_err(|e| TantivyBackendError::Backend(e.to_string()))?;
        let searcher = reader.searcher();
        let parser = QueryParser::for_index(&self.index, vec![self.text_field]);
        let parsed = parser
            .parse_query(query)
            .map_err(|e| TantivyBackendError::Query(e.to_string()))?;
        let top = searcher
            .search(&parsed, &TopDocs::with_limit(limit))
            .map_err(|e| TantivyBackendError::Backend(e.to_string()))?;
        let mut hits = Vec::with_capacity(top.len());
        for (score, addr) in top {
            let retrieved: TantivyDocument = searcher
                .doc(addr)
                .map_err(|e| TantivyBackendError::Backend(e.to_string()))?;
            let source_hex_val = retrieved
                .get_first(self.source_field)
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let page = retrieved
                .get_first(self.page_field)
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32;
            let reliable = retrieved
                .get_first(self.reliable_field)
                .and_then(|v| v.as_u64())
                .unwrap_or(0)
                != 0;
            hits.push(SearchHit {
                source: decode_source_hex(source_hex_val),
                page,
                reliable,
                score_milli: (score as f64 * 1000.0) as i64,
            });
        }
        Ok(hits)
    }

    /// Approximate on-disk index size in bytes — for settings visibility
    /// (size-budgeted, inspectable, ADR-019 §3).
    pub fn disk_size_bytes(&self, dir: &Path) -> u64 {
        std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| entry.metadata().ok())
            .filter(|meta| meta.is_file())
            .map(|meta| meta.len())
            .sum()
    }
}

/// Flush every record currently staged in `staging` into `index`, then clear
/// the staging area. Connects the bounded in-memory accumulation layer to
/// the persisted backend — the staging byte ceiling is enforced entirely
/// before this point, so anything reaching here has already passed it.
/// [ADR-019 §3]
pub fn flush_staged(
    staging: &mut crate::cross_document::IndexStaging,
    index: &mut CrossDocumentIndex,
) -> Result<usize, TantivyBackendError> {
    let mut count = 0;
    for record in staging.records() {
        index.upsert(record)?;
        count += 1;
    }
    staging.clear();
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_api::extract::{PageTextModel, TextLine};

    fn temp_index_dir(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pdf-platform-tantivy-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn model(page: u32, text: &str) -> PageTextModel {
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
            reliable: true,
            char_count: text.chars().count() as u32,
            has_structure: false,
        }
    }

    #[test]
    fn upsert_and_search_round_trip() {
        let dir = temp_index_dir("upsert-search");
        let mut index = CrossDocumentIndex::open_or_create(&dir).unwrap();
        let record = IndexRecord {
            source: [1; 16],
            revision: 1,
            page: 0,
            text: "the quick brown fox".into(),
            reliable: true,
        };
        index.upsert(&record).unwrap();

        let hits = index.search("quick", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].source, [1; 16]);
        assert_eq!(hits[0].page, 0);
        assert!(hits[0].reliable);

        assert!(index.search("nonexistent_term_xyz", 10).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn upsert_replaces_prior_revision_for_same_page() {
        let dir = temp_index_dir("revision-replace");
        let mut index = CrossDocumentIndex::open_or_create(&dir).unwrap();
        let source = [2; 16];
        index
            .upsert(&IndexRecord {
                source,
                revision: 1,
                page: 0,
                text: "old stale wording".into(),
                reliable: true,
            })
            .unwrap();
        index
            .upsert(&IndexRecord {
                source,
                revision: 2,
                page: 0,
                text: "new updated wording".into(),
                reliable: false,
            })
            .unwrap();

        assert!(index.search("stale", 10).unwrap().is_empty());
        let hits = index.search("updated", 10).unwrap();
        assert_eq!(hits.len(), 1, "only the newer revision should remain");
        assert!(!hits[0].reliable);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn remove_source_deletes_every_page_for_that_source_only() {
        let dir = temp_index_dir("remove-source");
        let mut index = CrossDocumentIndex::open_or_create(&dir).unwrap();
        let kept = [3; 16];
        let removed = [4; 16];
        index
            .upsert(&IndexRecord {
                source: removed,
                revision: 1,
                page: 0,
                text: "gone page zero".into(),
                reliable: true,
            })
            .unwrap();
        index
            .upsert(&IndexRecord {
                source: removed,
                revision: 1,
                page: 1,
                text: "gone page one".into(),
                reliable: true,
            })
            .unwrap();
        index
            .upsert(&IndexRecord {
                source: kept,
                revision: 1,
                page: 0,
                text: "unrelated surviving text".into(),
                reliable: true,
            })
            .unwrap();

        index.remove_source(removed).unwrap();

        assert!(index.search("gone", 10).unwrap().is_empty());
        let hits = index.search("surviving", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].source, kept);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn flush_staged_writes_only_what_staging_accepted() {
        use crate::cross_document::IndexStaging;

        let dir = temp_index_dir("flush-staged");
        let mut index = CrossDocumentIndex::open_or_create(&dir).unwrap();
        // Tiny budget: the second page's text won't fit and IndexStaging
        // must reject it before it ever reaches the Tantivy backend.
        let mut staging = IndexStaging::new(10).unwrap();
        let source = [5; 16];
        staging.ingest(source, 1, &model(0, "fits")).unwrap();
        assert!(staging
            .ingest(source, 1, &model(1, "this text is far too long to fit"))
            .is_err());

        let flushed = flush_staged(&mut staging, &mut index).unwrap();
        assert_eq!(flushed, 1, "only the accepted record should flush");
        assert!(staging.is_empty(), "staging clears after flush");

        let hits = index.search("fits", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].page, 0);
        assert!(
            index.search("far too long", 10).unwrap().is_empty(),
            "the budget-rejected page must never reach the index"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn disk_size_reports_nonzero_after_writes() {
        let dir = temp_index_dir("disk-size");
        let mut index = CrossDocumentIndex::open_or_create(&dir).unwrap();
        index
            .upsert(&IndexRecord {
                source: [6; 16],
                revision: 1,
                page: 0,
                text: "some indexed content for size accounting".into(),
                reliable: true,
            })
            .unwrap();
        assert!(index.disk_size_bytes(&dir) > 0);
        let _ = std::fs::remove_dir_all(dir);
    }
}
