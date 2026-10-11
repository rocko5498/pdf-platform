//! A save must never leave the destination truncated or half-written.
//! [SDS §3.4, §10.5, FR-REC-2, NFR-REL-4, MET-REL-2, T-5]
//!
//! Build the worker first (`cargo build -p worker-main`). [T-11]

mod common;

use common::{numbered_pdf, scratch, worker_path};
use coordinator::document::DocumentCoordinator;

fn stamp(coord: &mut DocumentCoordinator) {
    coord
        .apply_stamp(&pdf_model::stamp::Stamp {
            text: "REVIEWED".into(),
            ..pdf_model::stamp::Stamp::default()
        })
        .expect("stamp");
}

#[test]
fn a_failed_save_leaves_the_destination_exactly_as_it_was() {
    let dir = scratch("save-atomic-fail");
    let doc = dir.join("doc.pdf");
    std::fs::write(&doc, numbered_pdf(2)).expect("write");
    let before = std::fs::read(&doc).expect("read");

    let mut coord = DocumentCoordinator::open(&worker_path(), &doc).expect("open");
    stamp(&mut coord);

    // Block the staging file so the save cannot complete. Writing straight onto
    // the destination, as the old code did, would not notice and would have
    // truncated the document.
    std::fs::create_dir(dir.join("doc.pdf.pdfplatform-saving")).expect("block staging path");
    let outcome = coord.save_incremental(&doc);
    let _ = coord.close();

    assert!(outcome.is_err(), "the save was expected to fail");
    assert_eq!(std::fs::read(&doc).expect("read"), before, "a failed save altered the document");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_in_place_save_replaces_the_file_and_leaves_no_staging_file() {
    let dir = scratch("save-atomic-ok");
    let doc = dir.join("doc.pdf");
    std::fs::write(&doc, numbered_pdf(2)).expect("write");
    let original_len = std::fs::metadata(&doc).expect("meta").len();

    let mut coord = DocumentCoordinator::open(&worker_path(), &doc).expect("open");
    stamp(&mut coord);
    coord.save_incremental(&doc).expect("save in place");
    let _ = coord.close();

    assert!(std::fs::metadata(&doc).expect("meta").len() > original_len, "nothing was appended");
    assert!(!dir.join("doc.pdf.pdfplatform-saving").exists(), "staging file left behind");

    let mut reopened = DocumentCoordinator::open(&worker_path(), &doc).expect("reopen");
    let text: String = reopened
        .get_page_text(0)
        .expect("extract")
        .lines
        .iter()
        .map(|l| l.text.as_str())
        .collect();
    let _ = reopened.close();
    assert!(text.contains("REVIEWED"), "the saved file lost the stamp: {text:?}");
    let _ = std::fs::remove_dir_all(&dir);
}
