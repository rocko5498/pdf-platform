//! Worker session lifecycle: spawn, poll, death, inspect, respawn. [SDS §10.1, §3.1, ADR-008]
//!
//! M0: multi-process inspect + kill → respawn with document re-inherit.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::broker::BrokeredFile;
use protocol::events::{CoordinatorEvent, WorkerDeathReason};
use protocol::inspect::{decode_summary, StructuralSummary};
use protocol::transport::{TransportError, WorkerTransport as _};
use sandbox::spawn::{spawn_worker, spawn_worker_with_file, WorkerChild};

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
    /// Protocol / codec failure (inspect reply).
    Protocol(String),
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionError::Io(e) => write!(f, "session io: {e}"),
            SessionError::InvalidState(s) => write!(f, "session invalid state: {s}"),
            SessionError::Protocol(s) => write!(f, "session protocol: {s}"),
        }
    }
}

impl std::error::Error for SessionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SessionError::Io(e) => Some(e),
            SessionError::InvalidState(_) | SessionError::Protocol(_) => None,
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

/// One worker-backed session (optionally owns a brokered document).
pub struct WorkerSession {
    id: u64,
    worker_exe: PathBuf,
    state: LiveState,
    /// Z0-owned document; re-inherited on each spawn/respawn. [SDS §10.1 step 2]
    doc: Option<BrokeredFile>,
}

impl WorkerSession {
    /// Spawn a worker and attach it to a new session (no document).
    pub fn spawn(worker_exe: &Path) -> Result<Self, SessionError> {
        let child = spawn_worker(worker_exe)?;
        Ok(Self {
            id: NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed),
            worker_exe: worker_exe.to_path_buf(),
            state: LiveState::Alive { child },
            doc: None,
        })
    }

    /// Spawn a worker with a brokered document via inherited FD/HANDLE. [SDS §3.1, GR-1]
    ///
    /// Takes ownership of `doc` so the session can re-inherit on respawn.
    pub fn spawn_with_document(
        worker_exe: &Path,
        doc: BrokeredFile,
    ) -> Result<Self, SessionError> {
        let child = spawn_worker_with_file(worker_exe, doc.file(), &[])?;
        Ok(Self {
            id: NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed),
            worker_exe: worker_exe.to_path_buf(),
            state: LiveState::Alive { child },
            doc: Some(doc),
        })
    }

    /// Request a structural summary from the worker (`inspect` frame).
    pub fn inspect(&mut self) -> Result<StructuralSummary, SessionError> {
        self.send(b"inspect")?;
        let body = self.recv_frame(Duration::from_secs(30))?;
        decode_summary(&body).map_err(|e| SessionError::Protocol(e.to_string()))
    }

    /// Local session id (stable across respawn).
    pub fn session_id(&self) -> u64 {
        self.id
    }

    /// Whether the worker is currently considered alive.
    pub fn is_alive(&self) -> bool {
        matches!(self.state, LiveState::Alive { .. })
    }

    /// Whether this session owns a brokered document.
    pub fn has_document(&self) -> bool {
        self.doc.is_some()
    }

    /// Path used for (re)spawn of the worker binary.
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

    /// Receive one frame with timeout (tests / inspect).
    pub fn recv_frame(&mut self, timeout: Duration) -> Result<Vec<u8>, SessionError> {
        match &mut self.state {
            LiveState::Alive { child } => child
                .transport
                .recv_timeout(timeout)
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

                match child.transport.recv_timeout(timeout) {
                    Ok(_frame) => Ok(vec![]),
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

    /// After death: spawn a fresh worker; re-inherit document if present. [SDS §10.1]
    ///
    /// Session id is unchanged. Does not replay overlays (none in M0).
    pub fn respawn(&mut self) -> Result<(), SessionError> {
        match &self.state {
            LiveState::Alive { .. } => {
                return Err(SessionError::InvalidState("respawn while alive"));
            }
            LiveState::Dead { .. } => {}
        }
        let child = if let Some(doc) = self.doc.as_ref() {
            spawn_worker_with_file(&self.worker_exe, doc.file(), &[])?
        } else {
            spawn_worker(&self.worker_exe)?
        };
        self.state = LiveState::Alive { child };
        Ok(())
    }

    /// Alias for [`Self::respawn`] when no document is attached (ping-only).
    pub fn respawn_ping_only(&mut self) -> Result<(), SessionError> {
        if self.doc.is_some() {
            return Err(SessionError::InvalidState(
                "use respawn() when a document is attached",
            ));
        }
        self.respawn()
    }
}

fn transport_to_session_err(e: TransportError) -> SessionError {
    match e {
        TransportError::Io(ioe) => SessionError::Io(ioe),
        other => SessionError::Io(io::Error::new(io::ErrorKind::Other, other.to_string())),
    }
}
