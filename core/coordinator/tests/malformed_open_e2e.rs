//! A malformed document must not take the worker with it. [SDS §10.6, PRIN-1, GR-8]
//!
//! `pdf-cos` already has a corrupt-file corpus and a fuzz sweep, and both run
//! in-process: a panic there is a visible test failure. The path a user's file
//! actually travels is different — coordinator spawns Z1, Z1 parses untrusted
//! bytes, and a panic in that process reaches the coordinator only as
//! "transport disconnected". Every defect of this class recorded in
//! `docs/milestone-exit-tracker.md` presented exactly that way: the OCR
//! underflow, the PDFium bind race, the `parse_ref_array_bytes` overrun.
//!
//! So this asserts the end-to-end property SDS §14's M1 exit criterion names —
//! "repair corpus opens without crashes" — rather than the in-process one:
//! opening a malformed file either succeeds (with leniency recorded) or fails
//! with a typed error, and the failure is never a dead worker.
//!
//! Fixtures are derived here from a valid document rather than committed, so
//! nothing with provenance or confidentiality questions enters the repo
//! (GIT-5), and the corruption is visible in the test rather than opaque bytes.

use std::io::Write;
use std::path::{Path, PathBuf};

use coordinator::document::DocumentCoordinator;
use coordinator::session::SessionError;

fn worker_path() -> PathBuf {
    let exe = std::env::current_exe().expect("current exe");
    let mut dir = exe.parent().expect("exe dir").to_path_buf();
    if dir.ends_with("deps") {
        dir.pop();
    }
    dir.join(if cfg!(windows) { "worker.exe" } else { "worker" })
}

fn valid_pdf() -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tools")
        .join("corpus-diff")
        .join("fixtures")
        .join("valid-1page.pdf");
    std::fs::read(path).expect("read valid fixture")
}

fn write_temp(name: &str, bytes: &[u8]) -> PathBuf {
    let path = std::env::temp_dir().join(format!("pdf-platform-malformed-{name}.pdf"));
    let mut file = std::fs::File::create(&path).expect("create fixture");
    file.write_all(bytes).expect("write fixture");
    path
}

/// Corruptions that a real-world "repair" corpus is made of.
fn corrupt_cases() -> Vec<(&'static str, Vec<u8>)> {
    let valid = valid_pdf();
    let mut cases: Vec<(&'static str, Vec<u8>)> = Vec::new();

    // Header intact, everything after the first object removed: the classic
    // truncated download.
    let cut = valid.len() / 2;
    cases.push(("truncated", valid[..cut].to_vec()));

    // startxref points past EOF.
    let mut bad_startxref = valid.clone();
    if let Some(at) = find(&bad_startxref, b"startxref") {
        bad_startxref.truncate(at);
        bad_startxref.extend_from_slice(b"startxref\n99999999\n%%EOF\n");
    }
    cases.push(("startxref-past-eof", bad_startxref));

    // No startxref at all.
    let mut no_startxref = valid.clone();
    if let Some(at) = find(&no_startxref, b"startxref") {
        no_startxref.truncate(at);
        no_startxref.extend_from_slice(b"%%EOF\n");
    }
    cases.push(("no-startxref", no_startxref));

    // xref keyword present, entries garbage.
    let mut garbage_entries = valid.clone();
    if let Some(at) = find(&garbage_entries, b"xref") {
        let end = (at + 40).min(garbage_entries.len());
        for byte in &mut garbage_entries[at + 4..end] {
            *byte = b'?';
        }
    }
    cases.push(("garbage-xref-entries", garbage_entries));

    // A stream whose /Length lies about its size.
    let mut lying_length = valid.clone();
    if let Some(at) = find(&lying_length, b"/Length") {
        let end = (at + 20).min(lying_length.len());
        let replacement = b"/Length 999999999 ";
        for (i, byte) in replacement.iter().enumerate() {
            if at + i < end {
                lying_length[at + i] = *byte;
            }
        }
    }
    cases.push(("lying-stream-length", lying_length));

    // Not a PDF at all, but named like one.
    cases.push(("not-a-pdf", b"this is plain text, not a document".to_vec()));

    // Empty file.
    cases.push(("empty", Vec::new()));

    // Header only.
    cases.push(("header-only", b"%PDF-1.7\n".to_vec()));

    cases
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[test]
fn malformed_documents_never_kill_the_worker() {
    let worker = worker_path();
    assert!(
        worker.is_file(),
        "worker binary missing at {} — the end-to-end path cannot be exercised",
        worker.display()
    );

    let cases = corrupt_cases();
    let mut answered_by_worker = 0usize;

    for (name, bytes) in &cases {
        let path = write_temp(name, bytes);
        let outcome = DocumentCoordinator::open(&worker, &path);
        let _ = std::fs::remove_file(&path);

        match outcome {
            Ok(coord) => {
                // Opening a damaged file is allowed — that is repair. What is
                // not allowed is claiming health it cannot know, so a repaired
                // open must still answer for itself without panicking. [GR-8]
                let _ = coord.page_count();
                answered_by_worker += 1;
            }
            // A protocol-level error means Z1 parsed the bytes, decided they
            // were unusable, and said so over a live transport — the worker
            // survived, which is the property under test.
            Err(SessionError::Protocol(message)) => {
                let lower = message.to_lowercase();
                assert!(
                    !lower.contains("disconnect")
                        && !lower.contains("broken pipe")
                        && !lower.contains("no process"),
                    "'{name}' killed the worker instead of failing honestly: {message}"
                );
                answered_by_worker += 1;
            }
            Err(other) => panic!(
                "'{name}' failed outside the worker ({other}) — the parse path was never                  exercised, so this case proves nothing"
            ),
        }
    }

    // Without this the suite could pass by failing every case before spawn.
    assert_eq!(
        answered_by_worker,
        cases.len(),
        "every corrupt case must reach Z1 and get an answer back"
    );
}
