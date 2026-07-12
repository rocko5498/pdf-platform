//! Platform `WorkerTransport` impls. [ADR-031, SDS §4.2, design 2026-07-12]
//!
//! | OS      | Mechanism in this slice                                      |
//! |---------|--------------------------------------------------------------|
//! | Unix    | `UnixStream::pair` (`AF_UNIX` + `SOCK_STREAM`)               |
//! | Windows | Connected `TcpStream` on `127.0.0.1` for in-process tests   |
//!
//! // ponytail: Windows production path is named-pipe inherit at `sandbox::spawn`
//! // (next slice). Defer `windows-sys` until that slice needs it (ADR-028).
//! // TCP loopback is only a test duplex stand-in — not the product IPC path.

use std::io;
use std::time::Duration;

use protocol::transport::{
    read_frame_into, write_frame, FrameDecoder, TransportError, WorkerTransport,
};

// ---------------------------------------------------------------------------
// Unix — AF_UNIX stream
// ---------------------------------------------------------------------------

#[cfg(unix)]
mod unix {
    use super::*;
    use std::os::unix::net::UnixStream;

    /// Unix domain socket transport. [ADR-031]
    pub struct UnixWorkerTransport {
        stream: UnixStream,
        decoder: FrameDecoder,
    }

    impl UnixWorkerTransport {
        fn new(stream: UnixStream) -> Self {
            Self {
                stream,
                decoder: FrameDecoder::new(),
            }
        }

        /// Connected pair for tests (no spawn required).
        pub fn pair() -> io::Result<(Self, Self)> {
            let (a, b) = UnixStream::pair()?;
            Ok((Self::new(a), Self::new(b)))
        }
    }

    impl WorkerTransport for UnixWorkerTransport {
        fn send(&mut self, frame: &[u8]) -> Result<(), TransportError> {
            write_frame(&mut self.stream, frame)
        }

        fn recv_timeout(&mut self, timeout: Duration) -> Result<Vec<u8>, TransportError> {
            // macOS may return EINVAL if the peer already closed the socket.
            if let Err(e) = self.stream.set_read_timeout(Some(timeout)) {
                return Err(map_dead_socket(e));
            }
            let result = read_frame_into(&mut self.stream, &mut self.decoder);
            let _ = self.stream.set_read_timeout(None);
            result
        }
    }

    fn map_dead_socket(e: io::Error) -> TransportError {
        match e.kind() {
            io::ErrorKind::InvalidInput
            | io::ErrorKind::NotConnected
            | io::ErrorKind::BrokenPipe
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::ConnectionAborted => TransportError::Disconnected,
            _ => e.into(),
        }
    }

    /// Create a connected transport pair for tests.
    pub fn pair() -> io::Result<(UnixWorkerTransport, UnixWorkerTransport)> {
        UnixWorkerTransport::pair()
    }
}

#[cfg(unix)]
pub use unix::{pair, UnixWorkerTransport};

// ---------------------------------------------------------------------------
// Windows — test duplex via TCP loopback (named pipes next slice)
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod windows {
    use super::*;
    use std::net::{TcpListener, TcpStream};

    /// Windows control-channel transport (test path: TCP loopback). [ADR-031]
    ///
    /// Production will use named pipes once `sandbox::spawn` lands.
    pub struct WindowsWorkerTransport {
        stream: TcpStream,
        decoder: FrameDecoder,
    }

    impl WindowsWorkerTransport {
        fn from_stream(stream: TcpStream) -> io::Result<Self> {
            stream.set_nodelay(true)?;
            Ok(Self {
                stream,
                decoder: FrameDecoder::new(),
            })
        }

        /// Connected pair for tests (no spawn required).
        pub fn pair() -> io::Result<(Self, Self)> {
            let listener = TcpListener::bind("127.0.0.1:0")?;
            let addr = listener.local_addr()?;
            let client = TcpStream::connect(addr)?;
            let (server, _) = listener.accept()?;
            Ok((Self::from_stream(server)?, Self::from_stream(client)?))
        }
    }

    impl WorkerTransport for WindowsWorkerTransport {
        fn send(&mut self, frame: &[u8]) -> Result<(), TransportError> {
            write_frame(&mut self.stream, frame)
        }

        fn recv_timeout(&mut self, timeout: Duration) -> Result<Vec<u8>, TransportError> {
            if let Err(e) = self.stream.set_read_timeout(Some(timeout)) {
                return Err(map_dead_socket(e));
            }
            let result = read_frame_into(&mut self.stream, &mut self.decoder);
            let _ = self.stream.set_read_timeout(None);
            result
        }
    }

    fn map_dead_socket(e: io::Error) -> TransportError {
        match e.kind() {
            io::ErrorKind::InvalidInput
            | io::ErrorKind::NotConnected
            | io::ErrorKind::BrokenPipe
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::ConnectionAborted => TransportError::Disconnected,
            _ => e.into(),
        }
    }

    /// Create a connected transport pair for tests.
    pub fn pair() -> io::Result<(WindowsWorkerTransport, WindowsWorkerTransport)> {
        WindowsWorkerTransport::pair()
    }
}

#[cfg(windows)]
pub use windows::{pair, WindowsWorkerTransport};

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn pair_echo() {
        let (mut a, mut b) = pair().expect("pair");
        let handle = thread::spawn(move || {
            let msg = b.recv_timeout(Duration::from_secs(2)).expect("recv");
            b.send(&msg).expect("echo");
        });
        a.send(b"tile-ping").expect("send");
        let reply = a.recv_timeout(Duration::from_secs(2)).expect("reply");
        assert_eq!(reply, b"tile-ping");
        handle.join().unwrap();
    }

    #[test]
    fn pair_timeout_when_idle() {
        let (mut a, _b) = pair().expect("pair");
        match a.recv_timeout(Duration::from_millis(80)) {
            Err(TransportError::Timeout) => {}
            other => panic!("expected Timeout, got {other:?}"),
        }
    }

    #[test]
    fn pair_disconnect_on_drop() {
        let (mut a, b) = pair().expect("pair");
        drop(b);
        // Peer closed → Disconnected. Some stacks need a send to notice.
        match a.recv_timeout(Duration::from_millis(500)) {
            Err(TransportError::Disconnected) => {}
            Err(TransportError::Timeout) | Err(TransportError::Io(_)) => {
                match a.send(b"x") {
                    Err(TransportError::Disconnected) | Err(TransportError::Io(_)) => {}
                    other => panic!("expected disconnect after drop, got {other:?}"),
                }
            }
            other => panic!("expected Disconnected/Timeout/Io, got {other:?}"),
        }
    }
}
