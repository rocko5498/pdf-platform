//! `--json` gives a script something it can parse, and is refused where there
//! is no JSON form instead of being ignored. [FR-CLI-3, US-DEV-3, GR-8, T-12]
//!
//! These run the real binary and read what it prints.

use std::path::PathBuf;
use std::process::Command;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/corpus-diff/fixtures")
        .join(name)
}

fn run(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_pdf-platform"))
        .args(args)
        .output()
        .expect("run the cli");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn the_summary_is_one_json_document_with_the_real_page_count() {
    let file = fixture("valid-3page.pdf");
    let (code, stdout, _) = run(&[file.to_str().unwrap(), "--json"]);
    assert_eq!(code, 0);
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).expect("stdout is JSON");
    assert_eq!(value["pages"], 3);
    assert_eq!(value["acroform"], false);
    assert!(value["leniency"].is_array());
}

#[test]
fn revisions_are_listed_as_json() {
    let file = fixture("valid-3page.pdf");
    let (code, stdout, _) = run(&["revisions", file.to_str().unwrap(), "--json"]);
    assert_eq!(code, 0);
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).expect("stdout is JSON");
    let revisions = value["revisions"].as_array().expect("an array");
    assert_eq!(revisions.len(), 1);
    assert_eq!(revisions[0]["number"], 1);
}

#[test]
fn a_command_without_a_json_form_refuses_instead_of_printing_prose() {
    let file = fixture("valid-3page.pdf");
    let (code, stdout, stderr) = run(&["rotate", file.to_str().unwrap(), "--json"]);
    assert_eq!(code, 1, "usage error exit code");
    assert!(stdout.is_empty(), "nothing may be printed to stdout: {stdout:?}");
    assert!(stderr.contains("--json is not supported by `rotate`"), "{stderr}");
}

#[test]
fn without_the_flag_the_output_is_unchanged_prose() {
    let file = fixture("valid-3page.pdf");
    let (code, stdout, _) = run(&[file.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert!(stdout.starts_with("Pages:"), "{stdout:?}");
}

/// A two-page document with a note (with an author and text) on page 2.
fn document_with_a_note() -> Vec<u8> {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>",
        "<< /Type /Page /Parent 2 0 R >>",
        "<< /Type /Page /Parent 2 0 R /Annots [5 0 R] >>",
        "<< /Type /Annot /Subtype /Text /T (Ben) /Contents (Check the figure) /Rect [0 0 1 1] >>",
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n{body}\nendobj\n", i + 1).bytes());
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).bytes());
    for o in offsets {
        out.extend(format!("{o:010} 00000 n \n").bytes());
    }
    out.extend(
        format!(
            "trailer\n<</Size {} /Root 1 0 R>>\nstartxref\n{xref}\n%%EOF",
            objects.len() + 1
        )
        .bytes(),
    );
    out
}

#[test]
fn comments_are_listed_as_text_and_as_json_with_their_page() {
    let dir = std::env::temp_dir().join(format!("pdfp-comments-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("note.pdf");
    std::fs::write(&file, document_with_a_note()).unwrap();

    let (code, stdout, _) = run(&["comments", file.to_str().unwrap(), "--json"]);
    assert_eq!(code, 0);
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).expect("stdout is JSON");
    let first = &value["comments"][0];
    assert_eq!(first["page"], 2);
    assert_eq!(first["author"], "Ben");
    assert_eq!(first["text"], "Check the figure");

    let (code, prose, _) = run(&["comments", file.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert!(prose.contains("p.2 Text by Ben"), "{prose}");
    assert!(prose.contains("Check the figure"), "{prose}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn comment_filters_narrow_the_listing_through_the_real_binary() {
    let dir = std::env::temp_dir().join(format!("pdfp-cfilter-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("note.pdf");
    std::fs::write(&file, document_with_a_note()).unwrap();
    let count = |extra: &[&str]| -> usize {
        let mut args = vec!["comments", file.to_str().unwrap(), "--json"];
        args.extend_from_slice(extra);
        let (code, stdout, _) = run(&args);
        assert_eq!(code, 0);
        let value: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
        value["comments"].as_array().unwrap().len()
    };
    assert_eq!(count(&[]), 1);
    assert_eq!(count(&["--author", "ben"]), 1);
    assert_eq!(count(&["--author", "Ana"]), 0);
    assert_eq!(count(&["--page", "1"]), 0);
    assert_eq!(count(&["--type", "Text", "--page", "2"]), 1);
    let (code, _, stderr) = run(&["comments", file.to_str().unwrap(), "--page", "two"]);
    assert_eq!(code, 1);
    assert!(stderr.contains("--page must be a page number"), "{stderr}");
    let _ = std::fs::remove_dir_all(&dir);
}
