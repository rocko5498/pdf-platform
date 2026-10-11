//! Searching a long document must not keep every page's text for the whole
//! session, and must still find the page it is looking for. [GR-7, ADR-011,
//! FR-SRCH-1, T-12]
//!
//! Build the worker first (`cargo build -p worker-main`). [T-11]

mod common;

use common::{numbered_pdf, scratch, worker_path};
use coordinator::document::DocumentCoordinator;

#[test]
fn a_search_past_the_cache_bound_still_finds_the_page_and_stays_bounded() {
    let pages = text_extract::MAX_CACHED_PAGES as u32 + 80;
    let dir = scratch("bounded-text-cache");
    let doc = dir.join("long.pdf");
    std::fs::write(&doc, numbered_pdf(pages)).expect("write");

    let mut coord = DocumentCoordinator::open(&worker_path(), &doc).expect("open");
    // The marker of the last page (markers are numbered from 1): every earlier
    // page must be read to reach it.
    let last = pages - 1;
    let query = format!("PAGEMARK{pages}");
    let results = coord.find_in_document(&query).expect("find");
    let cached = coord.text_cache_page_count();
    let _ = coord.close();
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(results.len(), 1, "the last page was not found");
    assert_eq!(results[0].page_index, last);
    assert!(
        cached <= text_extract::MAX_CACHED_PAGES,
        "{cached} pages are cached, over the bound of {}",
        text_extract::MAX_CACHED_PAGES
    );
    assert!(cached > 0);
}
