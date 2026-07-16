//! Extraction correctness suite (unit). [ADR-019, SDS §14 M2 exit]
//!
//! Covers ligatures, soft hyphen, CJK, RTL, reliability flagging.
//! Full multi-engine corpus remains a separate gate with PDFium fixtures.

use engine_api::extract::{PageTextModel, TextLine};
use search::{find_all, FindOptions};
use text_extract::TextExtractionService;

fn line(i: u32, text: &str, y: f32) -> TextLine {
    TextLine {
        index: i,
        text: text.into(),
        x: 0.0,
        y,
        width: 200.0,
        height: 12.0,
        spans: vec![],
    }
}

fn model(lines: Vec<TextLine>, reliable: bool) -> PageTextModel {
    let char_count = lines.iter().map(|l| l.text.len() as u32).sum();
    PageTextModel {
        page_index: 0,
        lines,
        reliable,
        char_count,
        has_structure: false,
    }
}

struct StaticEngine {
    pages: Vec<PageTextModel>,
}

impl engine_api::extract::Extract for StaticEngine {
    fn extract_page(
        &self,
        page_index: u32,
    ) -> Result<PageTextModel, engine_api::extract::ExtractError> {
        self.pages
            .get(page_index as usize)
            .cloned()
            .ok_or(engine_api::extract::ExtractError::PageOutOfRange {
                requested: page_index,
                page_count: self.pages.len() as u32,
            })
    }
    fn page_count(&self) -> u32 {
        self.pages.len() as u32
    }
}

#[test]
fn suite_ligatures_fold_for_find() {
    let m = model(vec![line(0, "ﬁle ﬂag", 0.0)], true);
    let hits = find_all(&m, "file", &FindOptions::default());
    assert!(!hits.is_empty());
}

#[test]
fn suite_soft_hyphen_elided() {
    let m = model(vec![line(0, "soft\u{00AD}hyphen", 0.0)], true);
    assert_eq!(find_all(&m, "softhyphen", &FindOptions::default()).len(), 1);
}

#[test]
fn suite_cjk_match() {
    let m = model(vec![line(0, "日本語テスト", 0.0)], true);
    assert_eq!(find_all(&m, "日本", &FindOptions::default()).len(), 1);
}

#[test]
fn suite_rtl_arabic_match() {
    let m = model(vec![line(0, "مرحبا بالعالم", 0.0)], true);
    assert_eq!(find_all(&m, "مرحبا", &FindOptions::default()).len(), 1);
}

#[test]
fn suite_unreliable_flag_visible_to_search_agg() {
    let eng = StaticEngine {
        pages: vec![
            model(vec![line(0, "good", 0.0)], true),
            model(vec![line(0, "garbled", 0.0)], false),
        ],
    };
    let mut svc = TextExtractionService::new();
    svc.get_page(0, &eng).unwrap();
    svc.get_page(1, &eng).unwrap();
    assert!(!svc.get_cached(1).unwrap().reliable);
    // Search on unreliable page still works; aggregate flags unreliability
    let agg = text_extract::SearchResult::from_service(&svc, "garbled", 2);
    assert_eq!(agg.total_matches, 1);
    assert!(agg.has_unreliable);
}

#[test]
fn suite_service_cache_revision_invalidation() {
    let eng = StaticEngine {
        pages: vec![model(vec![line(0, "once", 0.0)], true)],
    };
    let mut svc = TextExtractionService::new();
    svc.get_page(0, &eng).unwrap();
    assert_eq!(svc.cached_page_count(), 1);
    svc.invalidate();
    assert_eq!(svc.cached_page_count(), 0);
    assert_eq!(svc.revision(), 1);
}

#[test]
fn suite_unreliable_page_find_still_returns_hits() {
    // Reliability flagging must not invent empty results. [ADR-019, GR-8]
    let m = model(vec![line(0, "broken ToUnicode still has glyphs", 0.0)], false);
    let hits = find_all(&m, "glyphs", &FindOptions::default());
    assert_eq!(hits.len(), 1);
    assert!(!m.reliable);
}

#[test]
fn suite_case_insensitive_ascii() {
    let m = model(vec![line(0, "Hello WORLD", 0.0)], true);
    let opts = FindOptions {
        case_sensitive: false,
        ..FindOptions::default()
    };
    assert!(!find_all(&m, "world", &opts).is_empty());
}
