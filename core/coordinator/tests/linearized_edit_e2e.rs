//! A linearized ("fast web view") document opens, stamps and saves through the
//! product path. [FR-VIEW-2, FR-STAMP, SDS §3.1, ISO 32000-1 Annex F]
//!
//! Real worker, real IPC, a real file produced by `qpdf --linearize`.
//!
//! What this does **not** do: discriminate the cross-reference fix. With the
//! forward-`/Prev` rule disabled these tests still pass, because the worker
//! takes its page count and renders from the engine. The test that fails
//! without the fix is `pdf-cos/tests/linearized.rs`, which reads the same file
//! through `scan_bytes`. This file is kept as coverage that the whole path
//! survives a linearized document, which nothing else exercised. [T-10, T-11]

mod common;

use common::{scratch, worker_path};
use coordinator::document::DocumentCoordinator;

fn fixture() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("pdf-cos")
        .join("tests")
        .join("fixtures")
        .join("linearized-3page.pdf")
}

#[test]
fn a_linearized_document_opens_with_all_its_pages() {
    let mut coord = DocumentCoordinator::open(&worker_path(), &fixture()).expect("open");
    let pages = coord.page_count();
    let _ = coord.close();
    assert_eq!(pages, 3);
}

#[test]
fn a_linearized_document_can_be_stamped_and_saved() {
    let dir = scratch("linearized-stamp");
    let stamped = dir.join("stamped.pdf");

    let mut coord = DocumentCoordinator::open(&worker_path(), &fixture()).expect("open");
    coord
        .apply_stamp(&pdf_model::stamp::Stamp {
            text: "REVIEWED".into(),
            ..pdf_model::stamp::Stamp::default()
        })
        .expect("stamp a page of a linearized document");
    coord.save_incremental(&stamped).expect("save");
    let _ = coord.close();

    let mut reopened = DocumentCoordinator::open(&worker_path(), &stamped).expect("reopen");
    let pages = reopened.page_count();
    let joined: String = reopened
        .get_page_text(0)
        .expect("extract")
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect();
    let _ = reopened.close();

    assert_eq!(pages, 3, "saving must not lose pages");
    assert!(joined.contains("REVIEWED"), "the stamp must be on the reopened page: {joined:?}");

    let _ = std::fs::remove_dir_all(&dir);
}
