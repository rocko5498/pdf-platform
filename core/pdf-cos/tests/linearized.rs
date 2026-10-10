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
fn a_linearized_original_is_one_revision_and_an_update_makes_it_two() {
    // The first-page and main cross-reference sections of a linearized file are
    // one save. Counting them as two would tell the user that a freshly
    // optimized file had been edited. [FR-VER-1]
    use pdf_cos::revisions::list_revisions;
    use pdf_cos::scan::find_startxref;
    use std::io::Write as _;

    let original = fixture();
    let one = list_revisions(&original).expect("list");
    assert_eq!(one.len(), 1, "a linearized original is a single save: {one:?}");
    assert_eq!(one[0].bytes, original.len());

    // Append one incremental update the way an editor would.
    let mut updated = original.clone();
    let previous = find_startxref(&original).expect("startxref");
    let object_at = updated.len();
    write!(updated, "\n90 0 obj\n<< >>\nendobj\n").unwrap();
    let xref_at = updated.len();
    write!(
        updated,
        "xref\n90 1\n{object_at:010} 00000 n \ntrailer\n<< /Size 91 /Prev {previous} >>\nstartxref\n{xref_at}\n%%EOF\n"
    )
    .unwrap();

    let two = list_revisions(&updated).expect("list");
    assert_eq!(two.len(), 2, "the original plus one update: {two:?}");
    assert_eq!(two[0], one[0], "the original revision is unchanged by an append");
    assert_eq!(two[1].objects_written, 1);
    assert_eq!(two[0].bytes + two[1].bytes, updated.len(), "the revisions tile the file");
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
