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
