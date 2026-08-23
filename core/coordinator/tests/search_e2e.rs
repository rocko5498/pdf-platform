//! Search over a real document, through the real path. [FR-SRCH-1, FR-SRCH-2, MET-FEAT-4]
//!
//! `search`'s unit tests operate on hand-written strings, and `text-extract`'s
//! correctness suite drives a `StaticEngine`. Both are real tests of
//! normalization. Neither proves that a user typing a word into this product
//! finds it, because neither parses a PDF: the chain is extraction in the
//! worker → the text model → normalization → match locations → selection
//! geometry, and only the middle of it was covered.
//!
//! That is the same gap that hid OCR recognizing nothing and the tile test
//! passing without an engine: assertions that stop at the plumbing.
//!
//! The fixtures carry explicit `/ToUnicode` CMaps, so what a match *should* be
//! is fixed by the file rather than by engine behaviour.

use std::path::PathBuf;

use coordinator::document::DocumentCoordinator;

fn worker_path() -> PathBuf {
    let exe = std::env::current_exe().expect("current exe");
    let mut dir = exe.parent().expect("exe dir").to_path_buf();
    if dir.ends_with("deps") {
        dir.pop();
    }
    dir.join(if cfg!(windows) { "worker.exe" } else { "worker" })
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tools")
        .join("corpus-diff")
        .join("fixtures")
        .join(name)
}

fn open(name: &str) -> DocumentCoordinator {
    let worker = worker_path();
    assert!(worker.is_file(), "worker binary missing at {}", worker.display());
    let path = fixture(name);
    assert!(path.is_file(), "fixture missing: {}", path.display());
    DocumentCoordinator::open(&worker, &path)
        .unwrap_or_else(|error| panic!("open {}: {error}", path.display()))
}

#[test]
fn a_word_on_the_page_is_found_with_its_location() {
    let mut coord = open("text-latin.pdf");
    let results = coord.find_in_document("extraction").expect("search");
    let _ = coord.close();

    assert!(!results.is_empty(), "'extraction' is drawn on page 1 and was not found");
    let page = &results[0];
    assert_eq!(page.page_index, 0);
    assert!(page.reliable, "a base-14 Latin page must be reported reliable");
    assert!(!page.matches.is_empty(), "a page result with no matches is not a match");

    // FR-SRCH-2: a match must say where it is, or it cannot be navigated to.
    let hit = &page.matches[0];
    assert!(hit.char_len > 0, "zero-length match: {hit:?}");
}

#[test]
fn a_word_that_is_not_there_is_not_found() {
    // The other half. A search that matches everything is as broken as one
    // that matches nothing, and only one of the two fails loudly.
    let mut coord = open("text-latin.pdf");
    let results = coord.find_in_document("nonexistentquerystring").expect("search");
    let _ = coord.close();
    assert!(results.is_empty(), "found text that is not in the document: {results:?}");
}

#[test]
fn a_match_resolves_to_selection_geometry_on_the_page() {
    // A match the UI cannot draw is not usable. This is the step between
    // "found it" and "showed you", and nothing covered it. [FR-SRCH-2]
    let mut coord = open("text-latin.pdf");
    let results = coord.find_in_document("second").expect("search");
    assert!(!results.is_empty(), "'second' is on page 1");
    let hit = results[0].matches[0].clone();

    let box_ = coord
        .selection_boxes_for_match(0, hit.line_index, hit.char_offset, hit.char_len)
        .expect("a match must resolve to a selection box");
    let _ = coord.close();

    assert!(box_.width > 0.0, "selection box has no width: {box_:?}");
    assert!(box_.height > 0.0, "selection box has no height: {box_:?}");
    assert!(box_.x >= 0.0 && box_.x <= 612.0, "outside the page: {box_:?}");
    assert!(box_.y >= 0.0 && box_.y <= 792.0, "outside the page: {box_:?}");
}

#[test]
fn a_ligature_is_found_by_typing_the_plain_letters() {
    // The fixture draws a single glyph whose ToUnicode says U+FB01, followed
    // by "ne". A user types "fine" and must find it.
    //
    // Which layer does the folding is deliberately not asserted: PDFium
    // decomposes the ligature during extraction, and `search::normalize` folds
    // it too, so either alone would satisfy this. What matters — and what no
    // test covered — is that the user's plain letters reach the drawn
    // ligature form through the whole chain. [FR-SRCH-1]
    let mut coord = open("text-ligature.pdf");
    let results = coord.find_in_document("fine").expect("search");
    let _ = coord.close();
    assert!(
        !results.is_empty(),
        "typing 'fine' must match the drawn ligature form"
    );
}

#[test]
fn a_soft_hyphenated_word_is_found_as_written_without_it() {
    // The fixture draws "co<U+00AD>operate". A user types "cooperate".
    // Extraction must preserve the soft hyphen and normalization must elide
    // it — if either half is wrong, this fails. [FR-SRCH-1]
    let mut coord = open("text-soft-hyphen.pdf");
    let results = coord.find_in_document("cooperate").expect("search");
    let _ = coord.close();
    assert!(
        !results.is_empty(),
        "a soft hyphen inside a word must not stop the word being found"
    );
}

#[test]
fn text_on_an_unreliable_page_is_still_found_but_flagged() {
    // ADR-019 §4: we do not hide text we cannot vouch for, we mark it. A
    // caller that shows matches without checking `reliable` is the thing the
    // flag exists to make possible to fix — so the flag has to arrive.
    let mut coord = open("text-unreliable.pdf");
    let results = coord.find_in_document("\u{E000}").expect("search");
    let _ = coord.close();

    assert!(!results.is_empty(), "the page's text is present and searchable");
    assert!(
        !results[0].reliable,
        "a Private-Use page must reach search results marked unreliable"
    );
}
