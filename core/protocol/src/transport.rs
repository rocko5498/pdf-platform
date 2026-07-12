//! WorkerTransport trait — platform-native IPC abstraction. [ADR-031]
//!
//! Concrete impls live in the sandbox crate (Unix domain socket / Windows named pipe).
//! Wire-format (serialisation) is a separate concern from this trait. [ADR-031 §Wire-format]
// ponytail: stub — define trait + timeout recv at M0 implementation

use std::time::Duration;

/// Errors returned by the transport layer.
#[derive(Debug)]
pub enum TransportError {
    /// Remote end closed — treat as worker crash. [ADR-031, SDS §10.1]
    Disconnected,
    Timeout,
    Io(std::io::Error),
}

impl From<std::io::Error> for TransportError {
    fn from(e: std::io::Error) -> Self {
        TransportError::Io(e)
    }
}

/// Bidirectional message channel to/from a sandboxed worker. [ADR-031]
pub trait WorkerTransport: Send + 'static {
    /// Send raw frame. Non-blocking from caller's perspective.
    fn send(&mut self, frame: &[u8]) -> Result<(), TransportError>;

    /// Receive next frame with timeout. Returns `Disconnected` on EOF/broken-pipe.
    fn recv_timeout(&mut self, timeout: Duration) -> Result<Vec<u8>, TransportError>;
}
