//! A real file with object streams and a predictor-filtered cross-reference
//! stream, as written by `qpdf --object-streams=generate`. [FR-VIEW-2, SDS §3.1]
//!
//! Every modern producer filters its xref stream with `/Predictor 12`. The
//! reader ignored predictors, so such a file came out as `Pages: 0` and
//! `unknown xref entry type 15`; the earlier tests that called modern PDFs
//! "fixed" used hand-built streams with no predictor at all.

use pdf_cos::scan::scan_bytes;

fn fixture() -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("qpdf-objstm-3page.pdf");
    std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

#[test]
fn the_fixture_really_uses_a_predictor_and_object_streams() {
    // Otherwise the test below would pass for the wrong reason. [T-11]
    let bytes = fixture();
    let has = |needle: &[u8]| bytes.windows(needle.len()).any(|w| w == needle);
    assert!(has(b"/Predictor 12"), "no predictor-filtered stream in the fixture");
    assert!(has(b"/Type /XRef"), "no cross-reference stream in the fixture");
    assert!(has(b"/Type /ObjStm"), "no object stream in the fixture");
}

#[test]
fn a_real_predictor_filtered_document_reports_its_pages_and_no_repair() {
    let structure = scan_bytes(&fixture()).expect("a valid file");
    assert_eq!(structure.page_count, 3, "the page tree lives inside an object stream");
    assert!(
        structure.leniency.is_empty(),
        "nothing about this file needs repair: {:?}",
        structure.leniency
    );
}
