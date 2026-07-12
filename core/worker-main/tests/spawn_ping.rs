//! Integration: parent spawns `worker`, ping/echo, quit. [ADR-008, SDS §3.1, ADR-022]
//!
//! Cites: design/plan 2026-07-12-worker-spawn

use std::path::Path;
use std::time::Duration;

use protocol::transport::{TransportError, WorkerTransport};
use sandbox::spawn::spawn_worker;

fn worker_path() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_worker"))
}

#[test]
fn spawn_ping_echo_quit() {
    let mut w = spawn_worker(worker_path()).expect("spawn_worker");

    w.transport
        .send(b"ping")
        .expect("send ping");
    let reply = w
        .transport
        .recv_timeout(Duration::from_secs(5))
        .expect("recv pong");
    assert_eq!(reply, b"ping");

    w.transport.send(b"quit").expect("send quit");
    let status = w.child.wait().expect("wait");
    assert!(status.success(), "worker exit status: {status}");
}

#[test]
fn spawn_kill_surfaces_disconnect() {
    let mut w = spawn_worker(worker_path()).expect("spawn_worker");

    w.child.kill().expect("kill");
    let _ = w.child.wait();

    match w.transport.recv_timeout(Duration::from_secs(2)) {
        Err(TransportError::Disconnected) => {}
        Err(TransportError::Timeout) => {
            // Some stacks need a write to notice; try send.
            match w.transport.send(b"x") {
                Err(TransportError::Disconnected) | Err(TransportError::Io(_)) => {}
                other => panic!("expected disconnect after kill, got {other:?}"),
            }
        }
        Err(TransportError::Io(_)) => {}
        other => panic!("expected Disconnected after kill, got {other:?}"),
    }
}
