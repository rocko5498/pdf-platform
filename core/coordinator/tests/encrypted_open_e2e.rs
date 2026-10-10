//! Opening encrypted documents: every algorithm, with and without object
//! streams. [FR-VIEW-2, FR-SEC-1, ADR-005, GR-8]
//!
//! The existing encrypted-open test covers AES-256 with plain objects. A modern
//! encrypted PDF keeps its page tree in *encrypted* object streams, which the
//! structure scan cannot read, so it reported no pages and the viewer opened
//! the file as an empty document while PDFium had the real count. Real worker,
//! real qpdf-encrypted files.
//!
//! The worker binary is not rebuilt by `cargo test -p coordinator`; build it
//! first (`cargo build -p worker-main`). [T-11]

mod common;

use common::{numbered_pdf, qpdf_or_skip, scratch, worker_path};
use coordinator::broker::open_read_only;
use coordinator::session::WorkerSession;
use std::process::Command;

fn encrypt(dir: &std::path::Path, name: &str, flags: &[&str], object_streams: bool) -> std::path::PathBuf {
    let source = dir.join("plain.pdf");
    std::fs::write(&source, numbered_pdf(3)).expect("write source");
    let out = dir.join(format!("{name}.pdf"));

    let mut args: Vec<String> = Vec::new();
    // qpdf wants the weak-crypto acknowledgement outside the encryption group.
    if flags.contains(&"--allow-weak-crypto") {
        args.push("--allow-weak-crypto".into());
    }
    args.extend(["--encrypt".to_string(), "userpw".into(), "ownerpw".into()]);
    args.extend(flags.iter().filter(|f| **f != "--allow-weak-crypto").map(|f| f.to_string()));
    args.push("--".into());
    if object_streams {
        args.push("--object-streams=generate".into());
    }
    args.push(source.to_str().unwrap().into());
    args.push(out.to_str().unwrap().into());

    let status = Command::new("qpdf").args(&args).status().expect("run qpdf");
    // 0 is clean; 3 is "succeeded with warnings".
    assert!(matches!(status.code(), Some(0) | Some(3)), "qpdf could not write {name}: {args:?}");
    out
}

#[test]
fn every_encryption_algorithm_opens_with_all_its_pages_and_its_text() {
    if !qpdf_or_skip() {
        return;
    }
    let dir = scratch("encrypted-matrix");
    let algorithms: [(&str, &[&str]); 4] = [
        ("rc4-40", &["40", "--allow-weak-crypto"]),
        ("rc4-128", &["128", "--use-aes=n", "--allow-weak-crypto"]),
        ("aes-128", &["128", "--use-aes=y"]),
        ("aes-256", &["256"]),
    ];
    let mut failures = Vec::new();
    for (name, flags) in algorithms {
        for object_streams in [false, true] {
            let label = format!("{name}{}", if object_streams { "+objstm" } else { "" });
            let path = encrypt(&dir, &label, flags, object_streams);
            let brokered = open_read_only(&path).expect("broker open");
            let mut session =
                WorkerSession::spawn_with_document_password(&worker_path(), brokered, Some("userpw"))
                    .expect("spawn with password");
            let summary = session.inspect().expect("inspect");
            let text = session.extract_page(0).map(|m| m.full_text()).unwrap_or_default();

            if summary.page_count != 3 {
                failures.push(format!("{label}: {} pages, expected 3", summary.page_count));
            }
            if !text.contains("PAGEMARK1") {
                failures.push(format!("{label}: page 1 text was {text:?}"));
            }
            // A disagreement between the scan and the engine must be disclosed.
            if object_streams
                && !summary.leniency_events.iter().any(|e| e.contains("page-count-mismatch"))
            {
                failures.push(format!("{label}: the scan/engine disagreement was not recorded"));
            }
            if !object_streams && summary.leniency_events.iter().any(|e| e.contains("page-count-mismatch")) {
                failures.push(format!("{label}: a mismatch was recorded where there is none"));
            }
        }
    }
    assert!(failures.is_empty(), "encrypted documents opened wrongly:\n{}", failures.join("\n"));
    let _ = std::fs::remove_dir_all(&dir);
}