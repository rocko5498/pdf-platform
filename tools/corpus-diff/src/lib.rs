use std::path::Path;
use std::process::Command;

/// True if the `qpdf` binary can be found and run.
pub fn qpdf_available() -> bool {
    Command::new("qpdf").arg("--version").output().is_ok()
}

/// Run `qpdf --show-npages` on `path` and parse the page count from stdout.
///
/// qpdf's exit code alone is not a reliable success signal: a recoverable file
/// exits 3 (warnings) but still prints a valid count on stdout, while a truly
/// unreadable file exits 2 with empty stdout. So this parses stdout for an
/// integer regardless of exit status, and only errors when stdout has no
/// parseable number.
pub fn qpdf_page_count(path: &Path) -> Result<u32, String> {
    let output = Command::new("qpdf")
        .arg("--show-npages")
        .arg(path)
        .output()
        .map_err(|e| format!("failed to run qpdf: {e}"))?;

    match String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<u32>()
    {
        Ok(n) => Ok(n),
        Err(_) => Err(format!(
            "qpdf reported no page count: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )),
    }
}

/// Result of comparing our scanner's structural summary against qpdf for one file.
pub enum FixtureResult {
    Pass { file: String, page_count: u32 },
    Fail { file: String, reason: String },
}

/// Compare our scanner against qpdf for one file, on page count only (v1 scope).
pub fn compare_fixture(path: &Path) -> FixtureResult {
    let file = path
        .file_name()
        .expect("fixture path must have a file name")
        .to_string_lossy()
        .to_string();

    let ours = coordinator::inspect::inspect(path).map(|s| s.page_count);
    let theirs = qpdf_page_count(path);

    match (ours, theirs) {
        (Ok(o), Ok(q)) if o == q => FixtureResult::Pass {
            file,
            page_count: o,
        },
        (Ok(o), Ok(q)) => FixtureResult::Fail {
            file,
            reason: format!("page count mismatch: ours={o}, qpdf={q}"),
        },
        (Err(e), Ok(q)) => FixtureResult::Fail {
            file,
            reason: format!("ours=err: {e}, qpdf={q}"),
        },
        (Ok(o), Err(e)) => FixtureResult::Fail {
            file,
            reason: format!("ours={o}, qpdf=err: {e}"),
        },
        (Err(oe), Err(qe)) => FixtureResult::Fail {
            file,
            reason: format!("ours=err: {oe}, qpdf=err: {qe}"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join(name)
    }

    #[test]
    fn qpdf_page_count_matches_known_good_file() {
        if !qpdf_available() {
            eprintln!("skip: qpdf not on PATH");
            return;
        }
        assert_eq!(qpdf_page_count(&fixture("valid-1page.pdf")), Ok(1));
    }

    #[test]
    fn qpdf_page_count_matches_three_page_file() {
        if !qpdf_available() {
            eprintln!("skip: qpdf not on PATH");
            return;
        }
        assert_eq!(qpdf_page_count(&fixture("valid-3page.pdf")), Ok(3));
    }

    #[test]
    fn compare_fixture_passes_when_counts_agree() {
        if !qpdf_available() {
            eprintln!("skip: qpdf not on PATH");
            return;
        }
        match compare_fixture(&fixture("valid-3page.pdf")) {
            FixtureResult::Pass { page_count, .. } => assert_eq!(page_count, 3),
            FixtureResult::Fail { reason, .. } => panic!("expected Pass, got Fail: {reason}"),
        }
    }

    #[test]
    fn a_malformed_xref_is_repaired_rather_than_refused() {
        // This used to assert `ours=err` for `malformed-xref.pdf`: our scan
        // failed where qpdf recovered. SDS §10.4 puts qpdf-style
        // reconstruction in our COS layer, and `parse_xref_chain` now falls
        // back to it, so the document opens and the page count agrees with
        // qpdf's. Asserting the old outcome would be asserting the defect.
        // [SDS §10.4, FR-VIEW-2, AI-7]
        if !qpdf_available() {
            eprintln!("skip: qpdf not on PATH");
            return;
        }
        let malformed = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("malformed-xref.pdf");
        match compare_fixture(&malformed) {
            FixtureResult::Pass { page_count, .. } => {
                assert!(page_count > 0, "a repaired document must have pages")
            }
            FixtureResult::Fail { reason, .. } => {
                panic!("a damaged xref must be reconstructed, not refused: {reason}")
            }
        }
    }

    #[test]
    fn a_file_that_is_not_a_pdf_still_fails() {
        // The Fail path must keep a case that reaches it, or the comparison
        // reports Pass for everything and proves nothing. [T-11]
        if !qpdf_available() {
            eprintln!("skip: qpdf not on PATH");
            return;
        }
        let path = std::env::temp_dir().join(format!(
            "pdf-platform-corpus-not-a-pdf-{}.pdf",
            std::process::id()
        ));
        std::fs::write(&path, b"this is not a PDF at all, not even close
").unwrap();

        let outcome = compare_fixture(&path);
        let _ = std::fs::remove_file(&path);

        match outcome {
            FixtureResult::Fail { .. } => {}
            FixtureResult::Pass { .. } => panic!("a text file must not compare as a PDF"),
        }
    }
}
