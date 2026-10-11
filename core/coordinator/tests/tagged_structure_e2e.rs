//! Whether a page's text is reported as structured follows the catalog, through
//! the real worker. It was hard-coded false, so a tagged document looked
//! untagged to every consumer. [FR-A11Y-1, GR-8]
//!
//! Build the worker first (`cargo build -p worker-main`). [T-11]

mod common;

use common::{assemble, scratch, worker_path};
use coordinator::document::DocumentCoordinator;

fn one_page(catalog_extra: &str) -> Vec<u8> {
    let content = b"BT /F1 24 Tf 20 100 Td (Hello) Tj ET";
    assemble(&[
        format!("<< /Type /Catalog /Pages 2 0 R {catalog_extra} >>").into_bytes(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R /Resources << /Font << /F1 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >> >> >>".to_vec(),
        [
            format!("<< /Length {} >>\nstream\n", content.len()).as_bytes(),
            content,
            b"\nendstream",
        ]
        .concat(),
        b"<< /Type /StructTreeRoot >>".to_vec(),
    ])
}

fn structured(name: &str, bytes: &[u8]) -> bool {
    let dir = scratch(name);
    let path = dir.join("doc.pdf");
    std::fs::write(&path, bytes).expect("write");
    let mut coord = DocumentCoordinator::open(&worker_path(), &path).expect("open");
    let flag = coord.get_page_text(0).expect("extract").has_structure;
    // The open-time summary the shell reads must say the same thing.
    assert_eq!(coord.summary().is_tagged, flag, "summary and page text disagree");
    let _ = coord.close();
    let _ = std::fs::remove_dir_all(&dir);
    flag
}

#[test]
fn a_tagged_document_reports_structure_and_an_untagged_one_does_not() {
    assert!(structured(
        "tagged",
        &one_page("/MarkInfo << /Marked true >> /StructTreeRoot 5 0 R")
    ));
    assert!(!structured("untagged", &one_page("")));
}
