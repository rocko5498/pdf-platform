use std::path::PathBuf;

use corpus_diff::{compare_fixture, qpdf_available, FixtureResult};

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn test_fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures")
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

#[test]
fn malformed_fixture_fails() {
    if !qpdf_available() {
        eprintln!("skip: qpdf not on PATH");
        return;
    }
    match compare_fixture(&test_fixtures_dir().join("malformed-xref.pdf")) {
        FixtureResult::Fail { reason, .. } => {
            assert!(reason.contains("ours=err"), "unexpected reason: {reason}");
        }
        FixtureResult::Pass { .. } => panic!("expected Fail for malformed-xref.pdf"),
    }
}
