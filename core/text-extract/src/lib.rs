//! Canonical text extraction service. [ADR-019, SDS §2.2.9]
//!
//! Owns the per-page text model cache (revision-keyed) and serves all
//! consumers: find, selection, copy, accessibility export, and indexing.
//! This is the single-extraction invariant — no feature re-extracts. [ADR-019 §1]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::collections::HashMap;

use engine_api::extract::{Extract, ExtractError, PageTextModel};

pub mod compare;

/// Cache key for a page's text model: (page_index, revision).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct CacheKey {
    page_index: u32,
    revision: u64,
}

/// Text extraction service: extracts pages and caches the results. [ADR-019]
///
/// The service is revision-aware: when a document changes, the caller
/// bumps the revision and old entries become stale (evicted on next access).
pub struct TextExtractionService {
    cache: HashMap<CacheKey, PageTextModel>,
    current_revision: u64,
}

impl TextExtractionService {
    /// Create a new empty service.
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
            current_revision: 0,
        }
    }

    /// Notify the service that the document has changed.
    /// All cached entries become stale and will be re-extracted on next access.
    pub fn invalidate(&mut self) {
        self.current_revision += 1;
        self.cache.clear();
    }

    /// Get the text model for a page, extracting if not cached.
    ///
    /// Returns the cached model if available at the current revision,
    /// otherwise calls the engine to extract and caches the result.
    pub fn get_page(
        &mut self,
        page_index: u32,
        engine: &dyn Extract,
    ) -> Result<&PageTextModel, ExtractError> {
        let key = CacheKey { page_index, revision: self.current_revision };

        if !self.cache.contains_key(&key) {
            let model = engine.extract_page(page_index)?;
            self.cache.insert(key, model);
        }

        Ok(self.cache.get(&key).unwrap())
    }

    /// Get the text model for a page if already cached (no extraction).
    pub fn get_cached(&self, page_index: u32) -> Option<&PageTextModel> {
        let key = CacheKey { page_index, revision: self.current_revision };
        self.cache.get(&key)
    }

    /// Find all occurrences of a query across multiple pages.
    ///
    /// Returns results sorted by page, then line, then character offset.
    /// Only searches pages that have been extracted (cached).
    pub fn find_in_cached_pages(
        &self,
        query: &str,
        page_indices: &[u32],
    ) -> Vec<PageSearchResult> {
        if query.is_empty() {
            return Vec::new();
        }

        let mut results = Vec::new();
        for &page_idx in page_indices {
            if let Some(model) = self.get_cached(page_idx) {
                let matches = model.find_all(query);
                if !matches.is_empty() {
                    results.push(PageSearchResult {
                        page_index: page_idx,
                        matches,
                        reliable: model.reliable,
                    });
                }
            }
        }

        results
    }

    /// Current revision number.
    pub fn revision(&self) -> u64 {
        self.current_revision
    }

    /// Number of cached pages.
    pub fn cached_page_count(&self) -> usize {
        self.cache.len()
    }

    /// Clear the cache (e.g., on document close).
    pub fn clear(&mut self) {
        self.cache.clear();
    }

    /// Insert a pre-extracted page model into the cache. [ADR-019]
    ///
    /// Used by the coordinator when the model was produced by a Z1 worker
    /// (ExtractPage) rather than an in-process `Extract` engine.
    pub fn insert_model(&mut self, model: PageTextModel) {
        let key = CacheKey {
            page_index: model.page_index,
            revision: self.current_revision,
        };
        self.cache.insert(key, model);
    }
}

impl Default for TextExtractionService {
    fn default() -> Self {
        Self::new()
    }
}

/// Search results for a single page.
#[derive(Debug, Clone)]
pub struct PageSearchResult {
    /// Page index.
    pub page_index: u32,
    /// Matches on this page.
    pub matches: Vec<engine_api::extract::MatchLocation>,
    /// Whether the page's text layer is reliable.
    pub reliable: bool,
}

/// Aggregate search results across pages.
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// Per-page results.
    pub pages: Vec<PageSearchResult>,
    /// Total match count across all pages.
    pub total_matches: u32,
    /// Whether any searched page had an unreliable text layer.
    pub has_unreliable: bool,
}

impl SearchResult {
    /// Search across all cached pages.
    pub fn from_service(service: &TextExtractionService, query: &str, page_count: u32) -> Self {
        let page_indices: Vec<u32> = (0..page_count).collect();
        let pages = service.find_in_cached_pages(query, &page_indices);
        let total_matches = pages.iter().map(|p| p.matches.len() as u32).sum();
        let has_unreliable = pages.iter().any(|p| !p.reliable);

        Self {
            pages,
            total_matches,
            has_unreliable,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_api::extract::{TextLine, TextSpan};

    struct MockEngine {
        models: HashMap<u32, PageTextModel>,
    }

    impl MockEngine {
        fn with_model(page_index: u32, text: &str) -> Self {
            let model = PageTextModel {
                page_index,
                lines: vec![TextLine {
                    index: 0,
                    text: text.to_string(),
                    x: 0.0, y: 0.0, width: 100.0, height: 12.0,
                    spans: vec![TextSpan {
                        text: text.to_string(),
                        x: 0.0, y: 0.0, width: 100.0, height: 12.0,
                        line_index: 0, word_index: 0, is_structured: false,
                    }],
                }],
                reliable: true,
                char_count: text.len() as u32,
                has_structure: false,
            };
            let mut models = HashMap::new();
            models.insert(page_index, model);
            Self { models }
        }
    }

    impl Extract for MockEngine {
        fn extract_page(&self, page_index: u32) -> Result<PageTextModel, ExtractError> {
            self.models.get(&page_index)
                .cloned()
                .ok_or(ExtractError::EmptyPage)
        }

        fn page_count(&self) -> u32 {
            self.models.keys().max().map_or(0, |k| k + 1)
        }
    }

    #[test]
    fn service_caches_and_serves() {
        let engine = MockEngine::with_model(0, "Hello world");
        let mut service = TextExtractionService::new();

        let model = service.get_page(0, &engine).unwrap();
        assert_eq!(model.char_count, 11);
        assert_eq!(service.cached_page_count(), 1);

        // Second call uses cache.
        let model2 = service.get_page(0, &engine).unwrap();
        assert_eq!(model2.char_count, 11);
        assert_eq!(service.cached_page_count(), 1);
    }

    #[test]
    fn service_invalidate_clears_cache() {
        let engine = MockEngine::with_model(0, "Hello");
        let mut service = TextExtractionService::new();

        service.get_page(0, &engine).unwrap();
        assert_eq!(service.cached_page_count(), 1);

        service.invalidate();
        assert_eq!(service.cached_page_count(), 0);
        assert_eq!(service.revision(), 1);
    }

    #[test]
    fn find_across_pages() {
        let mut models = HashMap::new();
        models.insert(0, PageTextModel {
            page_index: 0,
            lines: vec![TextLine {
                index: 0, text: "Hello world".into(),
                x: 0.0, y: 0.0, width: 100.0, height: 12.0, spans: vec![],
            }],
            reliable: true, char_count: 11, has_structure: false,
        });
        models.insert(1, PageTextModel {
            page_index: 1,
            lines: vec![TextLine {
                index: 0, text: "The world is big".into(),
                x: 0.0, y: 0.0, width: 100.0, height: 12.0, spans: vec![],
            }],
            reliable: true, char_count: 16, has_structure: false,
        });
        let engine = MockEngine { models };
        let mut service = TextExtractionService::new();

        // Pre-cache both pages.
        service.get_page(0, &engine).unwrap();
        service.get_page(1, &engine).unwrap();

        let results = service.find_in_cached_pages("world", &[0, 1]);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].page_index, 0);
        assert_eq!(results[0].matches.len(), 1);
        assert_eq!(results[1].page_index, 1);
        assert_eq!(results[1].matches.len(), 1);
    }

    #[test]
    fn search_result_aggregation() {
        let engine = MockEngine::with_model(0, "test test test");
        let mut service = TextExtractionService::new();
        service.get_page(0, &engine).unwrap();

        let result = SearchResult::from_service(&service, "test", 1);
        assert_eq!(result.total_matches, 3);
        assert!(!result.has_unreliable);
    }
}
