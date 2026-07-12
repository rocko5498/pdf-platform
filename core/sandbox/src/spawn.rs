//! Worker process spawn + IPC channel establishment. [ADR-008, SDS §3.1 step 3, §10.1]
//!
//! **Call order (M0):** create listen end → spawn worker with connect info → accept →
//! return parent transport. Confinement is **not** applied here (next slices).
//!
//! ## Env contract (child)
//!
//! | OS      | Variable                 | Meaning                          |
//! |---------|--------------------------|----------------------------------|
//! | Unix    | `PDF_PLATFORM_IPC_SOCK`  | Path to `UnixListener` socket    |
//! | Windows | `PDF_PLATFORM_IPC_PORT`  | `127.0.0.1` TCP port to connect  |
//!
//! // ponytail: connect-after-bind avoids FD CLOEXEC / named-pipe deps for this
//! // slice. Product hardening = inherit FD/HANDLE before confinement (design).

use std::io;
use std::path::Path;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use protocol::transport::WorkerTransport;

/// Env var: Unix domain socket path the worker must connect to.
pub const ENV_IPC_SOCK: &str = "PDF_PLATFORM_IPC_SOCK";
/// Env var: TCP port on 127.0.0.1 the worker must connect to (Windows M0).
pub const ENV_IPC_PORT: &str = "PDF_PLATFORM_IPC_PORT";
/// Optional override for worker binary path (tests / packaging).
pub const ENV_WORKER_PATH: &str = "PDF_PLATFORM_WORKER_PATH";

static SPAWN_SEQ: AtomicU64 = AtomicU64::new(1);

/// Parent-side handle: framed IPC + child process. [SDS §10.1]
pub struct WorkerChild {
    /// Control channel to the worker.
    pub transport: Box<dyn WorkerTransport>,
    /// OS process; drop does not kill — call `kill` or `wait` explicitly.
    pub child: Child,
}

/// Spawn `worker_exe` and establish a framed control channel.
///
/// No sandbox lockdown is applied (slice 2). See module docs for env contract.
pub fn spawn_worker(worker_exe: &Path) -> io::Result<WorkerChild> {
    #[cfg(unix)]
    {
        return unix::spawn_worker(worker_exe);
    }
    #[cfg(windows)]
    {
        return windows::spawn_worker(worker_exe);
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = worker_exe;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "spawn_worker unsupported on this OS",
        ))
    }
}

/// Adopt the inherited/connect IPC end inside the worker process.
pub fn adopt_inherited() -> io::Result<Box<dyn WorkerTransport>> {
    #[cfg(unix)]
    {
        return unix::adopt_inherited();
    }
    #[cfg(windows)]
    {
        return windows::adopt_inherited();
    }
    #[cfg(not(any(unix, windows)))]
    {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "adopt_inherited unsupported on this OS",
        ))
    }
}

// ---------------------------------------------------------------------------
// Unix — UnixListener path
// ---------------------------------------------------------------------------

#[cfg(unix)]
mod unix {
    use super::*;
    use crate::transport::UnixWorkerTransport;
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;

    pub fn spawn_worker(worker_exe: &Path) -> io::Result<WorkerChild> {
        let seq = SPAWN_SEQ.fetch_add(1, Ordering::Relaxed);
        let sock_path: PathBuf = std::env::temp_dir().join(format!(
            "pdf-platform-ipc-{}-{}.sock",
            std::process::id(),
            seq
        ));
        let _ = std::fs::remove_file(&sock_path);

        let listener = UnixListener::bind(&sock_path)?;
        // Bound path must be cleaned up if spawn/accept fails.
        struct Unlink(PathBuf);
        impl Drop for Unlink {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.0);
            }
        }
        let unlink = Unlink(sock_path.clone());

        let child = Command::new(worker_exe)
            .env(ENV_IPC_SOCK, &sock_path)
            .env_remove(ENV_IPC_PORT)
            .spawn()
            .map_err(|e| {
                io::Error::new(
                    e.kind(),
                    format!("spawn {}: {e}", worker_exe.display()),
                )
            })?;

        // Child retries connect; parent blocks on accept.
        listener.set_nonblocking(false)?;
        let (stream, _) = listener.accept().map_err(|e| {
            io::Error::new(e.kind(), format!("accept worker IPC: {e}"))
        })?;
        drop(unlink); // socket file no longer needed once connected

        let transport = Box::new(UnixWorkerTransport::from_stream(stream));
        Ok(WorkerChild { transport, child })
    }

    pub fn adopt_inherited() -> io::Result<Box<dyn WorkerTransport>> {
        let path = std::env::var(ENV_IPC_SOCK).map_err(|_| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("missing env {ENV_IPC_SOCK}"),
            )
        })?;
        let stream = connect_with_retry(|| UnixStream::connect(&path), 50, 100)?;
        Ok(Box::new(UnixWorkerTransport::from_stream(stream)))
    }
}

// ---------------------------------------------------------------------------
// Windows — TCP loopback port
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod windows {
    use super::*;
    use crate::transport::WindowsWorkerTransport;
    use std::net::{TcpListener, TcpStream};

    pub fn spawn_worker(worker_exe: &Path) -> io::Result<WorkerChild> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();

        let child = Command::new(worker_exe)
            .env(ENV_IPC_PORT, port.to_string())
            .env_remove(ENV_IPC_SOCK)
            .spawn()
            .map_err(|e| {
                io::Error::new(
                    e.kind(),
                    format!("spawn {}: {e}", worker_exe.display()),
                )
            })?;

        let (stream, _) = listener.accept().map_err(|e| {
            io::Error::new(e.kind(), format!("accept worker IPC: {e}"))
        })?;
        let transport = Box::new(WindowsWorkerTransport::from_stream(stream)?);
        Ok(WorkerChild { transport, child })
    }

    pub fn adopt_inherited() -> io::Result<Box<dyn WorkerTransport>> {
        let port: u16 = std::env::var(ENV_IPC_PORT)
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("missing env {ENV_IPC_PORT}"),
                )
            })?
            .parse()
            .map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("bad {ENV_IPC_PORT}: {e}"),
                )
            })?;
        let addr = format!("127.0.0.1:{port}");
        let stream = connect_with_retry(|| TcpStream::connect(&addr), 50, 100)?;
        Ok(Box::new(WindowsWorkerTransport::from_stream(stream)?))
    }
}

/// Retry connect until parent is listening (spawn race).
fn connect_with_retry<T, F>(mut connect: F, attempts: u32, delay_ms: u64) -> io::Result<T>
where
    F: FnMut() -> io::Result<T>,
{
    let mut last = None;
    for i in 0..attempts {
        match connect() {
            Ok(v) => return Ok(v),
            Err(e) => {
                last = Some(e);
                if i + 1 < attempts {
                    thread::sleep(Duration::from_millis(delay_ms));
                }
            }
        }
    }
    Err(last.unwrap_or_else(|| {
        io::Error::new(io::ErrorKind::ConnectionRefused, "connect retries exhausted")
    }))
}
