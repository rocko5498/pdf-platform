//! `insert-pages` through the real binary and real qpdf. [FR-INSERT-1, T-12]

use std::path::PathBuf;
use std::process::Command;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tools/corpus-diff/fixtures")
}

fn qpdf_pages(file: &PathBuf) -> u32 {
    let out = Command::new("qpdf").arg("--show-npages").arg(file).output().unwrap();
    String::from_utf8_lossy(&out.stdout).trim().parse().unwrap()
}

#[test]
fn inserting_adds_the_pages_and_never_overwrites() {
    if Command::new("qpdf").arg("--version").output().is_err() {
        eprintln!("skipped: qpdf not on PATH");
        return;
    }
    let dir = std::env::temp_dir().join(format!("pdfp-insert-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("out.pdf");
    let run = |at: &str| {
        Command::new(env!("CARGO_BIN_EXE_pdf-platform"))
            .arg("insert-pages")
            .arg(fixtures().join("valid-3page.pdf"))
            .arg("--from")
            .arg(fixtures().join("valid-1page.pdf"))
            .args(["--at", at, "-o"])
            .arg(&out)
            .output()
            .unwrap()
    };
    let first = run("2");
    assert_eq!(first.status.code(), Some(0), "{}", String::from_utf8_lossy(&first.stderr));
    assert_eq!(qpdf_pages(&out), 4);

    // The same output again is refused, and the file is left as it was.
    let again = run("2");
    assert_eq!(again.status.code(), Some(1));
    assert_eq!(qpdf_pages(&out), 4);

    // Past the end of the document is refused with no output written.
    let _ = std::fs::remove_file(&out);
    let past = run("9");
    assert_eq!(past.status.code(), Some(1));
    assert!(!out.exists());
    let _ = std::fs::remove_dir_all(&dir);
}
