//! Multi-process structural inspect via broker + inherited handle. [SDS §3.1, GR-1]
//! Designs: worker-open-inspect + handle-inherit

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
        WorkerSession::spawn_with_document(worker_path(), brokered).expect("spawn with doc");

    let summary = session.inspect().expect("inspect");
    assert_eq!(summary.page_count, 1, "summary={summary:?}");
    assert!(session.is_alive());

    session.send(b"quit").expect("quit");
    // Allow clean exit.
    let _ = session.poll(Duration::from_secs(2));
}

#[test]
fn kill_respawn_reinspect() {
    // M0 exit criterion: kill-the-worker shows transparent respawn. [SDS §14, §10.1]
    let pdf = fixture_pdf();
    assert!(pdf.is_file(), "fixture missing: {}", pdf.display());

    let brokered = open_read_only(&pdf).expect("broker open");
    let mut session =
        WorkerSession::spawn_with_document(worker_path(), brokered).expect("spawn");

    let first = session.inspect().expect("inspect before kill");
    assert_eq!(first.page_count, 1);

    session.kill_worker().expect("kill");
    let death = session.poll(Duration::from_secs(2)).expect("poll death");
    assert_eq!(death.len(), 1, "expected WorkerDied, got {death:?}");
    assert!(!session.is_alive());
    assert!(session.has_document());

    let id = session.session_id();
    session.respawn().expect("respawn with document");
    assert!(session.is_alive());
    assert_eq!(session.session_id(), id, "session id stable across respawn");

    let second = session.inspect().expect("inspect after respawn");
    assert_eq!(second.page_count, first.page_count);
    assert_eq!(second.has_acroform, first.has_acroform);

    session.send(b"quit").expect("quit");
    let _ = session.poll(Duration::from_secs(2));
}
