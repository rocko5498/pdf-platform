use std::path::PathBuf;

use corpus_diff::{compare_fixture, qpdf_available, FixtureResult};

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn test_fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

#[test]
fn known_good_fixture_passes() {
    if !qpdf_available() {
        eprintln!("skip: qpdf not on PATH");
        return;
    }
    match compare_fixture(&fixtures_dir().join("valid-1page.pdf")) {
        FixtureResult::Pass { page_count, .. } => assert_eq!(page_count, 1),
        FixtureResult::Fail { reason, .. } => panic!("expected Pass, got Fail: {reason}"),
    }
}

/// This asserted `ours=err`: our scan failed where qpdf recovered. SDS §10.4
/// puts qpdf-style reconstruction in our COS layer and `parse_xref_chain` now
/// falls back to it, so the document opens and agrees with qpdf on the page
/// count. Keeping the old assertion would assert the defect. [SDS §10.4, AI-7]
#[test]
fn a_malformed_xref_is_reconstructed_and_agrees_with_qpdf() {
    if !qpdf_available() {
        eprintln!("skip: qpdf not on PATH");
        return;
    }
    match compare_fixture(&test_fixtures_dir().join("malformed-xref.pdf")) {
        FixtureResult::Pass { page_count, .. } => {
            assert!(page_count > 0, "a reconstructed document must have pages");
        }
        FixtureResult::Fail { reason, .. } => {
            panic!("a damaged xref must be reconstructed, not refused: {reason}")
        }
    }
}
