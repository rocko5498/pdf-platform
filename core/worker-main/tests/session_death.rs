//! Integration: WorkerSession detects worker kill. [SDS §10.1, ADR-022]
//! Design: docs/superpowers/specs/2026-07-12-worker-session-design.md

use std::path::Path;
use std::time::Duration;

use coordinator::session::WorkerSession;
use protocol::events::{CoordinatorEvent, WorkerDeathReason};

fn worker_path() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_worker"))
}

#[test]
fn session_spawn_poll_timeout_stays_alive() {
    let mut s = WorkerSession::spawn(worker_path()).expect("spawn");
    assert!(s.is_alive());
    let events = s
        .poll(Duration::from_millis(80))
        .expect("poll");
    assert!(events.is_empty(), "unexpected events: {events:?}");
    assert!(s.is_alive());
    // Clean shutdown: kill so we don't leak processes if test ends.
    s.kill_worker().expect("kill");
}

#[test]
fn session_kill_emits_worker_died_once() {
    let mut s = WorkerSession::spawn(worker_path()).expect("spawn");
    let id = s.session_id();
    s.kill_worker().expect("kill");

    let events = s.poll(Duration::from_secs(2)).expect("poll after kill");
    assert_eq!(events.len(), 1, "expected one WorkerDied, got {events:?}");
    match &events[0] {
        CoordinatorEvent::WorkerDied { session_id, reason } => {
            assert_eq!(*session_id, id);
            match reason {
                WorkerDeathReason::ProcessExited { .. }
                | WorkerDeathReason::IpcDisconnected
                | WorkerDeathReason::Io { .. } => {}
            }
        }
        other => panic!("expected WorkerDied, got {other:?}"),
    }
    assert!(!s.is_alive());

    let again = s.poll(Duration::from_millis(50)).expect("second poll");
    assert!(
        again.is_empty(),
        "WorkerDied must be single-shot, got {again:?}"
    );
}

#[test]
fn session_respawn_ping_only() {
    let mut s = WorkerSession::spawn(worker_path()).expect("spawn");
    s.kill_worker().expect("kill");
    let _ = s.poll(Duration::from_secs(2)).expect("poll death");
    assert!(!s.is_alive());

    s.respawn_ping_only().expect("respawn");
    assert!(s.is_alive());

    s.send(b"ping").expect("send");
    // Drain via poll: worker echoes; poll ignores Ok frames in M0 — so use
    // a short poll loop is insufficient for reading echo. send path only
    // proves channel works if no error; do another send/quit.
    s.send(b"quit").expect("quit");
    // Worker exits; detect death.
    let mut saw_death = false;
    for _ in 0..20 {
        let ev = s.poll(Duration::from_millis(200)).expect("poll");
        if !ev.is_empty() {
            saw_death = true;
            break;
        }
        if !s.is_alive() {
            saw_death = true;
            break;
        }
    }
    assert!(saw_death || !s.is_alive(), "expected death after quit");
}
