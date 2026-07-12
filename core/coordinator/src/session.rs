//! Worker session lifecycle: spawn, poll, death detection. [SDS §10.1, ADR-008, ADR-021]
//!
//! M0 slice 3: detect worker death and emit `WorkerDied`. Full recovery
//! (re-broker file, re-parse, re-render) is **out of scope**.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use protocol::events::{CoordinatorEvent, WorkerDeathReason};
use protocol::transport::{TransportError, WorkerTransport as _};
use sandbox::spawn::{spawn_worker, WorkerChild};

static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);

/// Errors from session operations.
#[derive(Debug)]
pub enum SessionError {
    /// Underlying OS / spawn I/O.
    Io(io::Error),
    /// Operation invalid for current state (e.g. respawn while alive).
    InvalidState(
        /// Short static description of the invalid transition.
        &'static str,
    ),
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionError::Io(e) => write!(f, "session io: {e}"),
            SessionError::InvalidState(s) => write!(f, "session invalid state: {s}"),
        }
    }
}

impl std::error::Error for SessionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SessionError::Io(e) => Some(e),
            SessionError::InvalidState(_) => None,
        }
    }
}

impl From<io::Error> for SessionError {
    fn from(e: io::Error) -> Self {
        SessionError::Io(e)
    }
}

enum LiveState {
    Alive {
        child: WorkerChild,
    },
    Dead {
        reason: WorkerDeathReason,
        /// True after `WorkerDied` has been emitted once for this death.
        emitted: bool,
    },
}

/// One worker-backed session (M0: no document file yet).
pub struct WorkerSession {
    id: u64,
    worker_exe: PathBuf,
    state: LiveState,
}

impl WorkerSession {
    /// Spawn a worker and attach it to a new session.
    pub fn spawn(worker_exe: &Path) -> Result<Self, SessionError> {
        let child = spawn_worker(worker_exe)?;
        Ok(Self {
            id: NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed),
            worker_exe: worker_exe.to_path_buf(),
            state: LiveState::Alive { child },
        })
    }

    /// Local session id.
    pub fn session_id(&self) -> u64 {
        self.id
    }

    /// Whether the worker is currently considered alive.
    pub fn is_alive(&self) -> bool {
        matches!(self.state, LiveState::Alive { .. })
    }

    /// Path used for (re)spawn.
    pub fn worker_exe(&self) -> &Path {
        &self.worker_exe
    }

    /// Send a frame to the live worker (tests / ping).
    pub fn send(&mut self, frame: &[u8]) -> Result<(), SessionError> {
        match &mut self.state {
            LiveState::Alive { child } => child
                .transport
                .send(frame)
                .map_err(transport_to_session_err),
            LiveState::Dead { .. } => Err(SessionError::InvalidState("worker dead")),
        }
    }

    /// Fault-injection: kill the worker process. [ADR-022]
    pub fn kill_worker(&mut self) -> Result<(), SessionError> {
        match &mut self.state {
            LiveState::Alive { child } => {
                child.child.kill()?;
                let _ = child.child.wait();
                Ok(())
            }
            LiveState::Dead { .. } => Ok(()),
        }
    }

    /// Poll for liveness / inbound frames / death. [SDS §10.1]
    ///
    /// Does not block longer than roughly `timeout` for IPC receive.
    /// Emits at most one `WorkerDied` per death until `respawn_ping_only`.
    pub fn poll(&mut self, timeout: Duration) -> Result<Vec<CoordinatorEvent>, SessionError> {
        match &mut self.state {
            LiveState::Dead { reason, emitted } => {
                if *emitted {
                    return Ok(vec![]);
                }
                *emitted = true;
                let reason = reason.clone();
                Ok(vec![CoordinatorEvent::WorkerDied {
                    session_id: self.id,
                    reason,
                }])
            }
            LiveState::Alive { child } => {
                // 1) Process exit first.
                if let Some(status) = child.child.try_wait()? {
                    let reason = WorkerDeathReason::ProcessExited {
                        code: status.code(),
                    };
                    self.state = LiveState::Dead {
                        reason: reason.clone(),
                        emitted: true,
                    };
                    return Ok(vec![CoordinatorEvent::WorkerDied {
                        session_id: self.id,
                        reason,
                    }]);
                }

                // 2) IPC receive with timeout.
                match child.transport.recv_timeout(timeout) {
                    Ok(_frame) => {
                        // M0: unsolicited frames ignored (echo tests use send/recv helpers).
                        Ok(vec![])
                    }
                    Err(TransportError::Timeout) => Ok(vec![]),
                    Err(TransportError::Disconnected) => {
                        let reason = WorkerDeathReason::IpcDisconnected;
                        self.state = LiveState::Dead {
                            reason: reason.clone(),
                            emitted: true,
                        };
                        Ok(vec![CoordinatorEvent::WorkerDied {
                            session_id: self.id,
                            reason,
                        }])
                    }
                    Err(TransportError::FrameTooLarge { max, got }) => {
                        let reason = WorkerDeathReason::Io {
                            message: format!("frame too large: {got} (max {max})"),
                        };
                        self.state = LiveState::Dead {
                            reason: reason.clone(),
                            emitted: true,
                        };
                        Ok(vec![CoordinatorEvent::WorkerDied {
                            session_id: self.id,
                            reason,
                        }])
                    }
                    Err(TransportError::Io(e)) => {
                        let reason = WorkerDeathReason::Io {
                            message: e.to_string(),
                        };
                        self.state = LiveState::Dead {
                            reason: reason.clone(),
                            emitted: true,
                        };
                        Ok(vec![CoordinatorEvent::WorkerDied {
                            session_id: self.id,
                            reason,
                        }])
                    }
                }
            }
        }
    }

    /// After death: spawn a new worker (ping path only — no file broker). [design slice 3]
    pub fn respawn_ping_only(&mut self) -> Result<(), SessionError> {
        match &self.state {
            LiveState::Alive { .. } => {
                return Err(SessionError::InvalidState("respawn while alive"));
            }
            LiveState::Dead { .. } => {}
        }
        let child = spawn_worker(&self.worker_exe)?;
        self.state = LiveState::Alive { child };
        Ok(())
    }
}

fn transport_to_session_err(e: TransportError) -> SessionError {
    match e {
        TransportError::Io(ioe) => SessionError::Io(ioe),
        other => SessionError::Io(io::Error::new(io::ErrorKind::Other, other.to_string())),
    }
}
