use std::path::PathBuf;
use std::process::exit;

use corpus_diff::{compare_fixture, qpdf_available, FixtureResult};

fn main() {
    if !qpdf_available() {
        eprintln!("error: qpdf not found on PATH (required as the structural oracle)");
        exit(2);
    }

    let fixtures_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&fixtures_dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", fixtures_dir.display()))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "pdf"))
        .collect();
    paths.sort();

    let mut any_fail = false;
    for path in &paths {
        match compare_fixture(path) {
            FixtureResult::Pass { file, page_count } => {
                println!("PASS  {file:<24}(pages={page_count})");
            }
            FixtureResult::Fail { file, reason } => {
                any_fail = true;
                println!("FAIL  {file:<24}({reason})");
            }
        }
    }

    let total = paths.len();
    let verdict = if any_fail { "FAILURES" } else { "all passed" };
    println!("---\n{total} fixture(s) checked, {verdict}");

    exit(i32::from(any_fail));
}
