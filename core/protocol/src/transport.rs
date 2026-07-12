//! WorkerTransport trait — platform-native IPC abstraction. [ADR-031]
//!
//! Concrete OS impls live in the `sandbox` crate (Unix domain socket / Windows named pipe).
//! This module owns the trait, framing, and an in-process loopback for tests.
//! Wire-format (typed message serialisation) is a separate concern. [ADR-031]

use std::fmt;
use std::io::{self, Read, Write};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::time::Duration;

/// Hard cap on a single control-frame body (16 MiB). [design 2026-07-12, GR-7]
pub const MAX_FRAME: u32 = 16 * 1024 * 1024;

/// Errors returned by the transport layer.
#[derive(Debug)]
pub enum TransportError {
    /// Remote end closed — treat as worker crash. [ADR-031, SDS §10.1]
    Disconnected,
    /// `recv_timeout` elapsed without a complete frame.
    Timeout,
    /// Length prefix exceeded [`MAX_FRAME`].
    FrameTooLarge {
        /// Configured maximum body length.
        max: u32,
        /// Observed length prefix value.
        got: u32,
    },
    /// Underlying I/O failure.
    Io(io::Error),
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransportError::Disconnected => write!(f, "transport disconnected"),
            TransportError::Timeout => write!(f, "transport recv timeout"),
            TransportError::FrameTooLarge { max, got } => {
                write!(f, "frame too large: {got} bytes (max {max})")
            }
            TransportError::Io(e) => write!(f, "transport io: {e}"),
        }
    }
}

impl std::error::Error for TransportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            TransportError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for TransportError {
    fn from(e: io::Error) -> Self {
        match e.kind() {
            io::ErrorKind::UnexpectedEof
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::BrokenPipe
            | io::ErrorKind::ConnectionAborted => TransportError::Disconnected,
            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut => TransportError::Timeout,
            _ => TransportError::Io(e),
        }
    }
}

/// Bidirectional message channel to/from a sandboxed worker. [ADR-031]
///
/// Platform types never appear here — only in `sandbox`. Timeout receive is required
/// so the worker IPC thread can accept cancellations while rayon is busy (SDS §7, ADR-031 §6).
pub trait WorkerTransport: Send + 'static {
    /// Send one complete framed message (length-prefix + body).
    fn send(&mut self, frame: &[u8]) -> Result<(), TransportError>;

    /// Receive the next complete frame, or `Timeout` / `Disconnected`.
    fn recv_timeout(&mut self, timeout: Duration) -> Result<Vec<u8>, TransportError>;
}

// -- Framing ------------------------------------------------------------------

/// Encode `body` as `u32` LE length + body.
pub fn encode_frame(body: &[u8]) -> Result<Vec<u8>, TransportError> {
    let len = body.len();
    if len > MAX_FRAME as usize {
        return Err(TransportError::FrameTooLarge {
            max: MAX_FRAME,
            got: len as u32,
        });
    }
    let mut out = Vec::with_capacity(4 + len);
    out.extend_from_slice(&(len as u32).to_le_bytes());
    out.extend_from_slice(body);
    Ok(out)
}

/// Parse a length prefix; rejects oversize.
pub fn decode_length(prefix: [u8; 4]) -> Result<usize, TransportError> {
    let len = u32::from_le_bytes(prefix);
    if len > MAX_FRAME {
        return Err(TransportError::FrameTooLarge {
            max: MAX_FRAME,
            got: len,
        });
    }
    Ok(len as usize)
}

/// Write one framed message to `w`.
pub fn write_frame<W: Write>(w: &mut W, body: &[u8]) -> Result<(), TransportError> {
    let framed = encode_frame(body)?;
    w.write_all(&framed)?;
    w.flush()?;
    Ok(())
}

/// Incremental decoder for length-prefixed frames (handles partial reads).
#[derive(Debug, Default)]
pub struct FrameDecoder {
    buf: Vec<u8>,
}

impl FrameDecoder {
    /// Create an empty decoder.
    pub fn new() -> Self {
        Self { buf: Vec::new() }
    }

    /// Append raw bytes from the stream.
    pub fn push(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
    }

    /// Returns `Ok(Some(body))` when a full frame is available.
    pub fn try_next(&mut self) -> Result<Option<Vec<u8>>, TransportError> {
        if self.buf.len() < 4 {
            return Ok(None);
        }
        let mut prefix = [0u8; 4];
        prefix.copy_from_slice(&self.buf[..4]);
        let len = decode_length(prefix)?;
        let total = 4 + len;
        if self.buf.len() < total {
            return Ok(None);
        }
        let body = self.buf[4..total].to_vec();
        self.buf.drain(..total);
        Ok(Some(body))
    }

    /// Bytes held awaiting a complete frame.
    pub fn buffered_len(&self) -> usize {
        self.buf.len()
    }
}

/// Read one frame from `r`, using `decoder` to reassemble partial data.
///
/// Caller should set a read timeout on `r` when implementing `recv_timeout`.
pub fn read_frame_into<R: Read>(
    r: &mut R,
    decoder: &mut FrameDecoder,
) -> Result<Vec<u8>, TransportError> {
    let mut chunk = [0u8; 8192];
    loop {
        if let Some(body) = decoder.try_next()? {
            return Ok(body);
        }
        match r.read(&mut chunk) {
            Ok(0) => {
                // Peer closed (mid-frame or clean).
                return Err(TransportError::Disconnected);
            }
            Ok(n) => decoder.push(&chunk[..n]),
            Err(e) => return Err(e.into()),
        }
    }
}

// -- Loopback (in-process) ----------------------------------------------------

/// One end of an in-process duplex channel. [ADR-031 testable seam]
pub struct LoopbackTransport {
    tx: Sender<Vec<u8>>,
    rx: Receiver<Vec<u8>>,
}

impl LoopbackTransport {
    /// Create a connected pair. Dropping one end causes `Disconnected` on the other.
    pub fn pair() -> (Self, Self) {
        let (a_tx, a_rx) = mpsc::channel::<Vec<u8>>();
        let (b_tx, b_rx) = mpsc::channel::<Vec<u8>>();
        (
            LoopbackTransport { tx: b_tx, rx: a_rx },
            LoopbackTransport { tx: a_tx, rx: b_rx },
        )
    }
}

impl WorkerTransport for LoopbackTransport {
    fn send(&mut self, frame: &[u8]) -> Result<(), TransportError> {
        if frame.len() > MAX_FRAME as usize {
            return Err(TransportError::FrameTooLarge {
                max: MAX_FRAME,
                got: frame.len() as u32,
            });
        }
        self.tx
            .send(frame.to_vec())
            .map_err(|_| TransportError::Disconnected)
    }

    fn recv_timeout(&mut self, timeout: Duration) -> Result<Vec<u8>, TransportError> {
        match self.rx.recv_timeout(timeout) {
            Ok(v) => Ok(v),
            Err(RecvTimeoutError::Timeout) => {
                // Distinguish empty-timeout from disconnected.
                match self.rx.try_recv() {
                    Err(TryRecvError::Disconnected) => Err(TransportError::Disconnected),
                    Err(TryRecvError::Empty) => Err(TransportError::Timeout),
                    Ok(v) => Ok(v),
                }
            }
            Err(RecvTimeoutError::Disconnected) => Err(TransportError::Disconnected),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn encode_decode_roundtrip() {
        let body = b"hello-worker";
        let framed = encode_frame(body).unwrap();
        assert_eq!(&framed[..4], &(body.len() as u32).to_le_bytes());
        let mut dec = FrameDecoder::new();
        dec.push(&framed);
        assert_eq!(dec.try_next().unwrap().unwrap(), body);
    }

    #[test]
    fn empty_body_ok() {
        let framed = encode_frame(b"").unwrap();
        assert_eq!(framed, [0, 0, 0, 0]);
        let mut dec = FrameDecoder::new();
        dec.push(&framed);
        assert_eq!(dec.try_next().unwrap().unwrap(), b"");
    }

    #[test]
    fn reject_oversize_encode() {
        let huge = vec![0u8; (MAX_FRAME as usize) + 1];
        match encode_frame(&huge) {
            Err(TransportError::FrameTooLarge { max, got }) => {
                assert_eq!(max, MAX_FRAME);
                assert_eq!(got, MAX_FRAME + 1);
            }
            other => panic!("expected FrameTooLarge, got {other:?}"),
        }
    }

    #[test]
    fn reject_oversize_length_prefix() {
        let bad = (MAX_FRAME + 1).to_le_bytes();
        assert!(matches!(
            decode_length(bad),
            Err(TransportError::FrameTooLarge { .. })
        ));
    }

    #[test]
    fn partial_reassembly() {
        let body = b"abcdef";
        let framed = encode_frame(body).unwrap();
        let mut dec = FrameDecoder::new();
        dec.push(&framed[..3]);
        assert!(dec.try_next().unwrap().is_none());
        dec.push(&framed[3..]);
        assert_eq!(dec.try_next().unwrap().unwrap(), body);
    }

    #[test]
    fn write_read_frame_cursor() {
        let mut buf = Cursor::new(Vec::new());
        write_frame(&mut buf, b"ping").unwrap();
        buf.set_position(0);
        let mut dec = FrameDecoder::new();
        let body = read_frame_into(&mut buf, &mut dec).unwrap();
        assert_eq!(body, b"ping");
    }

    #[test]
    fn loopback_roundtrip() {
        let (mut a, mut b) = LoopbackTransport::pair();
        a.send(b"req").unwrap();
        assert_eq!(b.recv_timeout(Duration::from_secs(1)).unwrap(), b"req");
        b.send(b"resp").unwrap();
        assert_eq!(a.recv_timeout(Duration::from_secs(1)).unwrap(), b"resp");
    }

    #[test]
    fn loopback_timeout() {
        let (mut a, _b) = LoopbackTransport::pair();
        match a.recv_timeout(Duration::from_millis(50)) {
            Err(TransportError::Timeout) => {}
            other => panic!("expected Timeout, got {other:?}"),
        }
    }

    #[test]
    fn loopback_disconnect() {
        let (mut a, b) = LoopbackTransport::pair();
        drop(b);
        match a.recv_timeout(Duration::from_millis(100)) {
            Err(TransportError::Disconnected) => {}
            other => panic!("expected Disconnected, got {other:?}"),
        }
        match a.send(b"x") {
            Err(TransportError::Disconnected) => {}
            other => panic!("expected Disconnected on send, got {other:?}"),
        }
    }
}
