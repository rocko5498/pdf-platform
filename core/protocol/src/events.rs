//! Events: coordinator → shell (in-process types for M0). [ADR-004, SDS §5]
//!
//! Wire codec (bincode/etc.) is deferred; these types are the semantic contract.
//! Transport still carries opaque frames between processes. [transport design]

/// Why a document worker is considered dead. [SDS §10.1]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerDeathReason {
    /// IPC channel closed / broken (peer gone).
    IpcDisconnected,
    /// OS process exited (`Child::try_wait`).
    ProcessExited {
        /// Exit code when available (None if signalled / unknown on platform).
        code: Option<i32>,
    },
    /// I/O or other failure treated as death (honest failure, not silent alive).
    Io {
        /// Display message only (no raw `io::Error` across FFI yet).
        message: String,
    },
}

/// Events produced by the coordinator for clients (shell, CLI, tests).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoordinatorEvent {
    /// Worker for a session is gone. [SDS §10.1 detection]
    WorkerDied {
        /// Local session id (not durable across app restarts).
        session_id: u64,
        /// How death was observed.
        reason: WorkerDeathReason,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_died_construct_and_match() {
        let e = CoordinatorEvent::WorkerDied {
            session_id: 7,
            reason: WorkerDeathReason::IpcDisconnected,
        };
        match e {
            CoordinatorEvent::WorkerDied {
                session_id,
                reason: WorkerDeathReason::IpcDisconnected,
            } => assert_eq!(session_id, 7),
            other => panic!("unexpected {other:?}"),
        }
    }
}
