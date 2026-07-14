//! Platform `WorkerTransport` impls. [ADR-031 / transport design, SDS §4.2]
//!
//! | OS      | In-process `pair()`              | Spawn path (`spawn` module)        |
//! |---------|----------------------------------|------------------------------------|
//! | Unix    | `UnixStream::pair`               | `UnixListener` + path in env       |
//! | Windows | `NamedPipeServer`/`Client` pair  | Named pipe + pipe name in env      |

use std::io;
use std::time::Duration;

#[allow(unused_imports)]
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
        /// Wrap an already-connected stream.
        pub fn from_stream(stream: UnixStream) -> Self {
            Self {
                stream,
                decoder: FrameDecoder::new(),
            }
        }

        /// Connected pair for tests (no spawn required).
        pub fn pair() -> io::Result<(Self, Self)> {
            let (a, b) = UnixStream::pair()?;
            Ok((Self::from_stream(a), Self::from_stream(b)))
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
// Windows — Named pipes [ADR-031]
//
// Named pipes are local-only (no network exposure), support bidirectional
// framing, and integrate with Windows handle inheritance for the spawn
// path. The pipe server (coordinator) creates the pipe; the client
// (worker) connects via CreateFileW.
//
// SECURITY: AppContainer sandbox (ADR-016) is deferred; pipes use default
// security for M0. Confinement will add a per-AppContainer security
// descriptor before enforcement.
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod windows {
    use super::*;
    use std::os::windows::io::{AsRawHandle, RawHandle};

    // -- Raw FFI for pipe functions --------------------------------------------
    // Using raw FFI instead of windows-sys features to avoid import path
    // complexity and keep the sandbox crate's dependency surface minimal.

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateNamedPipeW(
            lpname: *const u16,
            dwopenmode: u32,
            dwpipemode: u32,
            nmaxinstances: u32,
            noutbuffersize: u32,
            ninbuffersize: u32,
            ndefaulttimeout: u32,
            lpsecurityattributes: *const core::ffi::c_void,
        ) -> RawHandle;

        fn ConnectNamedPipe(
            hnamedpipe: RawHandle,
            lpoverlapped: *mut core::ffi::c_void,
        ) -> i32;

        fn DisconnectNamedPipe(hnamedpipe: RawHandle) -> i32;

        fn CreateFileW(
            lpfilename: *const u16,
            dwdesiredaccess: u32,
            dwsharemode: u32,
            lpsecurityattributes: *const core::ffi::c_void,
            dwcreationdisposition: u32,
            dwflagsandattributes: u32,
            htemplatefile: RawHandle,
        ) -> RawHandle;

        fn WriteFile(
            hfile: RawHandle,
            lpbuffer: *const u8,
            nnumberofbytestowrite: u32,
            lpnumberofbyteswritten: *mut u32,
            lpoverlapped: *mut core::ffi::c_void,
        ) -> i32;

        fn ReadFile(
            hfile: RawHandle,
            lpbuffer: *mut u8,
            nnumberofbytestoread: u32,
            lpnumberofbytesread: *mut u32,
            lpoverlapped: *mut core::ffi::c_void,
        ) -> i32;

        fn CancelSynchronousIo(hthread: RawHandle) -> i32;

        fn CloseHandle(hObject: RawHandle) -> i32;

        fn GetLastError() -> u32;
    }

    const INVALID_HANDLE_VALUE: RawHandle = -1isize as RawHandle;
    const NULL_HANDLE: RawHandle = 0 as RawHandle;

    // Pipe open modes
    const PIPE_ACCESS_DUPLEX: u32 = 0x00000003;
    const PIPE_TYPE_BYTE: u32 = 0x00000000;
    const PIPE_READMODE_BYTE: u32 = 0x00000000;
    const PIPE_WAIT: u32 = 0x00000000;

    // File access
    const GENERIC_READ: u32 = 0x80000000;
    const GENERIC_WRITE: u32 = 0x40000000;
    const OPEN_EXISTING: u32 = 3;

    // Error codes
    const ERROR_PIPE_CONNECTED: u32 = 535;
    const ERROR_BROKEN_PIPE: u32 = 109;

    // SAFETY: A pipe HANDLE is a kernel object; it is safe to send between
    // threads (it is not a user-space pointer). This wrapper exists solely
    // to satisfy Rust's auto-trait inference for raw pointers.
    #[derive(Debug)]
    struct PipeHandle(RawHandle);

    unsafe impl Send for PipeHandle {}

    impl PipeHandle {
        fn is_valid(&self) -> bool {
            self.0 != NULL_HANDLE && self.0 != INVALID_HANDLE_VALUE
        }

        fn close(&mut self) {
            if self.is_valid() {
                // SAFETY: we own this handle and it is valid.
                unsafe { CloseHandle(self.0) };
                self.0 = NULL_HANDLE;
            }
        }
    }

    /// Windows named-pipe transport (server side). [ADR-031]
    ///
    /// Creates the pipe, waits for a client to connect, then operates as a
    /// bidirectional framed channel.
    pub struct NamedPipeServer {
        handle: PipeHandle,
        decoder: FrameDecoder,
    }

    // SAFETY: PipeHandle wraps a kernel HANDLE which is Send.
    unsafe impl Send for NamedPipeServer {}

    impl NamedPipeServer {
        /// Create a new named pipe server and wait for one client connection.
        ///
        /// `name` is the full pipe path, e.g. `\\.\pipe\pdf-platform-1-1`.
        pub fn new(name: &str) -> io::Result<Self> {
            let wide_name = to_wide(name);

            // SAFETY: wide_name is null-terminated; null SA uses default
            // security (sufficient for M0; AppContainer SA added at
            // confinement hardening).
            let handle = unsafe {
                CreateNamedPipeW(
                    wide_name.as_ptr(),
                    PIPE_ACCESS_DUPLEX,
                    PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                    1,       // max instances
                    65536,   // out buffer
                    65536,   // in buffer
                    0,       // default timeout
                    core::ptr::null(),
                )
            };
            if handle == INVALID_HANDLE_VALUE {
                return Err(io::Error::last_os_error());
            }

            // Wait for one client to connect.
            // SAFETY: handle is a valid named-pipe server handle.
            let ok = unsafe { ConnectNamedPipe(handle, core::ptr::null_mut()) };
            if ok == 0 {
                let err = unsafe { GetLastError() };
                if err != ERROR_PIPE_CONNECTED {
                    // SAFETY: handle is valid and we're about to return error.
                    unsafe { CloseHandle(handle) };
                    return Err(io::Error::new(
                        io::ErrorKind::Other,
                        format!("ConnectNamedPipe failed: error {err}"),
                    ));
                }
                // ERROR_PIPE_CONNECTED = client connected before we called
                // ConnectNamedPipe — this is normal, not an error.
            }

            Ok(Self {
                handle: PipeHandle(handle),
                decoder: FrameDecoder::new(),
            })
        }
    }

    impl WorkerTransport for NamedPipeServer {
        fn send(&mut self, frame: &[u8]) -> Result<(), TransportError> {
            write_pipe(self.handle.0, frame)
        }

        fn recv_timeout(&mut self, timeout: Duration) -> Result<Vec<u8>, TransportError> {
            recv_with_timeout(PipeHandle(self.handle.0), &mut self.decoder, timeout)
        }
    }

    impl Drop for NamedPipeServer {
        fn drop(&mut self) {
            if self.handle.is_valid() {
                // Disconnect the pipe before closing.
                // SAFETY: handle is valid.
                unsafe {
                    DisconnectNamedPipe(self.handle.0);
                }
                self.handle.close();
            }
        }
    }

    /// Windows named-pipe transport (client side). [ADR-031]
    ///
    /// Connects to an existing named pipe server.
    pub struct NamedPipeClient {
        handle: PipeHandle,
        decoder: FrameDecoder,
    }

    unsafe impl Send for NamedPipeClient {}

    impl NamedPipeClient {
        /// Connect to a named pipe server.
        pub fn connect(name: &str) -> io::Result<Self> {
            let wide_name = to_wide(name);

            // SAFETY: wide_name is null-terminated; we use GENERIC_READ|WRITE
            // for bidirectional communication.
            let handle = unsafe {
                CreateFileW(
                    wide_name.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    0,                       // no sharing
                    core::ptr::null(),       // default security
                    OPEN_EXISTING,
                    0,                       // default attributes
                    NULL_HANDLE,             // no template
                )
            };
            if handle == INVALID_HANDLE_VALUE {
                return Err(io::Error::last_os_error());
            }

            Ok(Self {
                handle: PipeHandle(handle),
                decoder: FrameDecoder::new(),
            })
        }
    }

    impl WorkerTransport for NamedPipeClient {
        fn send(&mut self, frame: &[u8]) -> Result<(), TransportError> {
            write_pipe(self.handle.0, frame)
        }

        fn recv_timeout(&mut self, timeout: Duration) -> Result<Vec<u8>, TransportError> {
            recv_with_timeout(PipeHandle(self.handle.0), &mut self.decoder, timeout)
        }
    }

    impl Drop for NamedPipeClient {
        fn drop(&mut self) {
            self.handle.close();
        }
    }

    // -- helpers ---------------------------------------------------------------

    fn to_wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Write a length-prefixed frame to a pipe handle.
    fn write_pipe(handle: RawHandle, frame: &[u8]) -> Result<(), TransportError> {
        let len = frame.len() as u32;
        let len_bytes = len.to_le_bytes();

        // SAFETY: handle is valid; buffers are valid for their lengths.
        unsafe {
            let mut written = 0u32;
            let ok = WriteFile(
                handle,
                len_bytes.as_ptr(),
                4,
                &mut written,
                core::ptr::null_mut(),
            );
            if ok == 0 || written != 4 {
                return Err(io::Error::last_os_error().into());
            }

            written = 0;
            let ok = WriteFile(
                handle,
                frame.as_ptr(),
                len,
                &mut written,
                core::ptr::null_mut(),
            );
            if ok == 0 || written != len {
                return Err(io::Error::last_os_error().into());
            }
        }
        Ok(())
    }

    /// Non-owning handle wrapper for passing to threads. Does NOT close the
    /// handle on drop — the caller is responsible for lifecycle.
    #[derive(Clone, Copy)]
    struct BorrowedHandle(RawHandle);

    unsafe impl Send for BorrowedHandle {}

    /// Read one frame from a pipe handle, blocking until data arrives.
    fn blocking_read_frame(
        handle: BorrowedHandle,
        decoder: &mut FrameDecoder,
    ) -> Result<Vec<u8>, TransportError> {
        loop {
            if let Some(frame) = decoder.try_next()? {
                return Ok(frame);
            }
            let mut buf = [0u8; 8192];
            // SAFETY: handle is valid; buf is writable for its length.
            let n = unsafe {
                let mut read = 0u32;
                let ok = ReadFile(
                    handle.0,
                    buf.as_mut_ptr(),
                    buf.len() as u32,
                    &mut read,
                    core::ptr::null_mut(),
                );
                if ok == 0 {
                    let err = GetLastError();
                    if err == ERROR_BROKEN_PIPE {
                        return Err(TransportError::Disconnected);
                    }
                    return Err(io::Error::from_raw_os_error(err as i32).into());
                }
                read as usize
            };
            if n == 0 {
                return Err(TransportError::Disconnected);
            }
            decoder.push(&buf[..n]);
        }
    }

    /// Receive a frame with a timeout using a reader thread + cancellation.
    ///
    /// Named pipe `ReadFile` blocks indefinitely, so we spawn a short-lived
    /// reader thread and use `CancelSynchronousIo` if the timeout expires.
    fn recv_with_timeout(
        handle: PipeHandle,
        decoder: &mut FrameDecoder,
        timeout: Duration,
    ) -> Result<Vec<u8>, TransportError> {
        // Fast path: data might already be buffered in the decoder.
        if let Some(frame) = decoder.try_next()? {
            return Ok(frame);
        }

        use std::sync::mpsc;

        let mut decoder_cell = Some(std::mem::take(decoder));
        let (tx, rx) = mpsc::channel();
        let borrowed = BorrowedHandle(handle.0);

        let thread_handle = std::thread::spawn(move || {
            let mut dec = decoder_cell.take().unwrap();
            let result = blocking_read_frame(borrowed, &mut dec);
            let _ = tx.send((result, dec));
        });

        let raw_thread = thread_handle.as_raw_handle();

        match rx.recv_timeout(timeout) {
            Ok((result, returned_decoder)) => {
                let _ = thread_handle.join();
                *decoder = returned_decoder;
                result
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                unsafe { CancelSynchronousIo(raw_thread) };
                let _ = thread_handle.join();
                // Decoder was not returned; create a fresh one.
                // Any buffered data in the old decoder is lost on timeout —
                // acceptable for M0; production would use overlapped I/O.
                Err(TransportError::Timeout)
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let _ = thread_handle.join();
                Err(TransportError::Disconnected)
            }
        }
    }

    /// Create a connected transport pair for tests using named pipes.
    pub fn pair() -> io::Result<(NamedPipeServer, NamedPipeClient)> {
        let id = std::process::id();
        let seq = TEST_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let name = format!("\\\\.\\pipe\\pdf-platform-test-{id}-{seq}");

        let name_clone = name.clone();
        let server = std::thread::spawn(move || NamedPipeServer::new(&name_clone));

        // Small delay to let the server create the pipe before we connect.
        std::thread::sleep(Duration::from_millis(10));
        let client = NamedPipeClient::connect(&name)?;

        let server = server
            .join()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "server thread panicked"))??;

        Ok((server, client))
    }

    static TEST_SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);
}

#[cfg(windows)]
pub use windows::{pair, NamedPipeClient, NamedPipeServer};

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
