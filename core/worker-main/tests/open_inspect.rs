//! Multi-process structural inspect via broker + worker. [SDS §3.1, FR-DIAG-2]
//! Design: docs/superpowers/specs/2026-07-12-worker-open-inspect-design.md

use std::path::{Path, PathBuf};
use std::time::Duration;

use coordinator::broker::open_read_only;
use coordinator::session::WorkerSession;

fn worker_path() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_worker"))
}

fn fixture_pdf() -> PathBuf {
    // core/worker-main -> repo root -> tools/corpus-diff/fixtures
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tools")
        .join("corpus-diff")
        .join("fixtures")
        .join("valid-1page.pdf")
}

#[test]
fn broker_spawn_inspect_one_page() {
    let pdf = fixture_pdf();
    assert!(
        pdf.is_file(),
        "fixture missing: {}",
        pdf.display()
    );

    let brokered = open_read_only(&pdf).expect("broker open");
    let mut session =
        WorkerSession::spawn_with_document(worker_path(), &brokered).expect("spawn with doc");

    let summary = session.inspect().expect("inspect");
    assert_eq!(summary.page_count, 1, "summary={summary:?}");
    assert!(session.is_alive());

    session.send(b"quit").expect("quit");
    // Allow clean exit.
    let _ = session.poll(Duration::from_secs(2));
}
