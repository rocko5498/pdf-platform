//! A real linearized file, as produced by `qpdf --linearize`. [FR-VIEW-2, SDS §3.1]
//!
//! The unit tests in `scan.rs` build the Annex F layout by hand. This reads the
//! real thing: the structural summary of a linearized document reported
//! `Pages: 0` and a spurious repair, because the chain walker refused the
//! first-page section's forward `/Prev`.

use pdf_cos::scan::scan_bytes;

fn fixture() -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("linearized-3page.pdf");
    std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

#[test]
fn the_fixture_really_is_linearized() {
    // Otherwise the test below would pass for the wrong reason. [T-11]
    let bytes = fixture();
    assert!(
        bytes[..bytes.len().min(1024)]
            .windows(b"/Linearized".len())
            .any(|w| w == b"/Linearized"),
        "linearized-3page.pdf has no /Linearized key in its first 1024 bytes"
    );
}

#[test]
fn a_real_linearized_document_reports_its_pages_and_no_repair() {
    let structure = scan_bytes(&fixture()).expect("a linearized file is a valid file");
    assert_eq!(structure.page_count, 3, "the page tree is in the main section");
    assert!(
        structure.leniency.is_empty(),
        "a linearized file needs no repair: {:?}",
        structure.leniency
    );
}
