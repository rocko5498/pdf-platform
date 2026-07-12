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

    match String::from_utf8_lossy(&output.stdout).trim().parse::<u32>() {
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
        (Ok(o), Ok(q)) if o == q => FixtureResult::Pass { file, page_count: o },
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
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures").join(name)
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
    fn compare_fixture_fails_on_our_scan_error() {
        if !qpdf_available() {
            eprintln!("skip: qpdf not on PATH");
            return;
        }
        let malformed = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("malformed-xref.pdf");
        match compare_fixture(&malformed) {
            FixtureResult::Fail { reason, .. } => assert!(reason.contains("ours=err")),
            FixtureResult::Pass { .. } => panic!("expected Fail for malformed-xref.pdf"),
        }
    }
}
