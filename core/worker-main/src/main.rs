//! Z1 worker binary. Never holds authoritative document state. [ADR-008, SDS §2.3]
//!
//! M0 slice 2: adopt IPC + echo frames. No PDF parse, no engine, no confinement.

use std::process::ExitCode;
use std::time::Duration;

use protocol::transport::{TransportError, WorkerTransport as _};
use sandbox::spawn::adopt_inherited;

fn main() -> ExitCode {
    let mut transport = match adopt_inherited() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("worker: adopt IPC failed: {e}");
            return ExitCode::from(1);
        }
    };

    // Echo loop until quit or peer disconnect. [design 2026-07-12 worker-spawn]
    loop {
        match transport.recv_timeout(Duration::from_secs(1)) {
            Ok(msg) if msg == b"quit" => break,
            Ok(msg) => {
                if let Err(e) = transport.send(&msg) {
                    eprintln!("worker: send failed: {e}");
                    return ExitCode::from(2);
                }
            }
            Err(TransportError::Timeout) => continue,
            Err(TransportError::Disconnected) => break,
            Err(e) => {
                eprintln!("worker: recv failed: {e}");
                return ExitCode::from(3);
            }
        }
    }
    ExitCode::SUCCESS
}
