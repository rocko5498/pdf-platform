//! Editing a real document whose cross-reference is predictor-filtered and
//! whose objects live in object streams. [FR-VIEW-2, FR-STAMP, FR-ORG-1, SDS §3.1]
//!
//! The file is `qpdf --object-streams=generate` output, the shape nearly every
//! modern producer writes. Before predictors were undone, stamping it failed
//! with "object 2 not found" and deleting a page reported a document of zero
//! pages, while PDFium rendered it perfectly. Unlike the hand-built modern
//! document in `modern_pdf_edit_e2e.rs`, this one carries a `/Predictor 12`.
//!
//! The worker binary is not rebuilt by `cargo test -p coordinator`; build it
//! first (`cargo build -p worker-main`). [T-11]

mod common;

use common::{scratch, worker_path};
use coordinator::document::DocumentCoordinator;

fn fixture() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("pdf-cos")
        .join("tests")
        .join("fixtures")
        .join("qpdf-objstm-3page.pdf")
}

#[test]
fn a_predictor_filtered_document_can_be_stamped_and_saved() {
    let dir = scratch("qpdf-objstm-stamp");
    let stamped = dir.join("stamped.pdf");

    let mut coord = DocumentCoordinator::open(&worker_path(), &fixture()).expect("open");
    coord
        .apply_stamp(&pdf_model::stamp::Stamp {
            text: "REVIEWED".into(),
            ..pdf_model::stamp::Stamp::default()
        })
        .expect("stamp a page whose objects live in a predictor-filtered object stream");
    coord.save_incremental(&stamped).expect("save");
    let _ = coord.close();

    // A second reader's verdict on the file we wrote, not ours. [FR-SAVE-1]
    if common::qpdf_or_skip() {
        let check = std::process::Command::new("qpdf").arg("--check").arg(&stamped).output().expect("run qpdf");
        let said = String::from_utf8_lossy(&check.stdout).to_string() + &String::from_utf8_lossy(&check.stderr);
        assert!(check.status.code() == Some(0), "qpdf --check: {said}");
    }

    let mut reopened = DocumentCoordinator::open(&worker_path(), &stamped).expect("reopen");
    let pages = reopened.page_count();
    let text: String = reopened
        .get_page_text(0)
        .expect("extract")
        .lines
        .iter()
        .map(|l| l.text.as_str())
        .collect();
    let _ = reopened.close();

    assert_eq!(pages, 3);
    assert!(text.contains("REVIEWED"), "the stamp must be on the reopened page: {text:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_page_of_a_predictor_filtered_document_can_be_deleted() {
    let dir = scratch("qpdf-objstm-delete");
    let edited = dir.join("edited.pdf");

    let mut coord = DocumentCoordinator::open(&worker_path(), &fixture()).expect("open");
    coord.delete_pages(&[1]).expect("delete page 2");
    coord.save_incremental(&edited).expect("save");
    let _ = coord.close();

    let mut reopened = DocumentCoordinator::open(&worker_path(), &edited).expect("reopen");
    let pages = reopened.page_count();
    let _ = reopened.close();
    assert_eq!(pages, 2, "one of three pages was deleted");
    let _ = std::fs::remove_dir_all(&dir);
}
