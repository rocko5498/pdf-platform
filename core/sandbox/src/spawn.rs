//! Worker process spawn + IPC + document handle inherit. [ADR-008, SDS §3.1, §10.1]
//!
//! **Call order (M0):** create IPC listen end → mark doc handle inheritable →
//! spawn worker → accept IPC → return parent transport.
//!
//! ## Env contract (child)
//!
//! | Variable | Meaning |
//! |----------|---------|
//! | `PDF_PLATFORM_IPC_SOCK` | Unix: path to parent `UnixListener` |
//! | `PDF_PLATFORM_IPC_PORT` | Windows: `127.0.0.1` TCP port |
//! | `PDF_PLATFORM_DOC_FD` | Unix: inherited document file descriptor |
//! | `PDF_PLATFORM_DOC_HANDLE` | Windows: inherited document HANDLE (integer) |
//!
//! Document path is **not** passed (GR-1 / SDS §3.1). IPC still uses
//! connect-after-bind (separate debt from document handle inherit).

use std::fs::File;
use std::io;
use std::path::Path;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

/// Env var: Unix domain socket path the worker must connect to.
pub const ENV_IPC_SOCK: &str = "PDF_PLATFORM_IPC_SOCK";
/// Env var: TCP port on 127.0.0.1 the worker must connect to (Windows M0).
pub const ENV_IPC_PORT: &str = "PDF_PLATFORM_IPC_PORT";
/// Optional override for worker binary path (tests / packaging).
pub const ENV_WORKER_PATH: &str = "PDF_PLATFORM_WORKER_PATH";
/// Unix: inherited document FD number.
pub const ENV_DOC_FD: &str = "PDF_PLATFORM_DOC_FD";
/// Windows: inherited document HANDLE as decimal integer.
pub const ENV_DOC_HANDLE: &str = "PDF_PLATFORM_DOC_HANDLE";

static SPAWN_SEQ: AtomicU64 = AtomicU64::new(1);

/// Parent-side handle: framed IPC + child process. [SDS §10.1]
pub struct WorkerChild {
    /// Control channel to the worker.
    pub transport: Box<dyn protocol::transport::WorkerTransport>,
    /// OS process; drop does not kill — call `kill` or `wait` explicitly.
    pub child: Child,
}

/// Spawn `worker_exe` and establish a framed control channel (no document).
pub fn spawn_worker(worker_exe: &Path) -> io::Result<WorkerChild> {
    spawn_worker_with_env(worker_exe, &[])
}

/// Like [`spawn_worker`], with extra environment variables for the child.
pub fn spawn_worker_with_env(
    worker_exe: &Path,
    extra_env: &[(&str, &str)],
) -> io::Result<WorkerChild> {
    spawn_impl(worker_exe, None, extra_env)
}

/// Spawn worker with an inheritable document file (no path string). [SDS §3.1]
///
/// `doc` must remain open until this function returns (parent keeps `BrokeredFile`).
pub fn spawn_worker_with_file(
    worker_exe: &Path,
    doc: &File,
    extra_env: &[(&str, &str)],
) -> io::Result<WorkerChild> {
    spawn_impl(worker_exe, Some(doc), extra_env)
}

/// Adopt the IPC end inside the worker process.
pub fn adopt_inherited() -> io::Result<Box<dyn protocol::transport::WorkerTransport>> {
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

/// Adopt the inherited document file, if the parent attached one.
pub fn adopt_document_file() -> io::Result<Option<File>> {
    #[cfg(unix)]
    {
        return unix::adopt_document_file();
    }
    #[cfg(windows)]
    {
        return windows::adopt_document_file();
    }
    #[cfg(not(any(unix, windows)))]
    {
        Ok(None)
    }
}

fn spawn_impl(
    worker_exe: &Path,
    doc: Option<&File>,
    extra_env: &[(&str, &str)],
) -> io::Result<WorkerChild> {
    #[cfg(unix)]
    {
        return unix::spawn_worker(worker_exe, doc, extra_env);
    }
    #[cfg(windows)]
    {
        return windows::spawn_worker(worker_exe, doc, extra_env);
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (worker_exe, doc, extra_env);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "spawn_worker unsupported on this OS",
        ))
    }
}

fn apply_extra_env(cmd: &mut Command, extra_env: &[(&str, &str)]) {
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
}

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

// ---------------------------------------------------------------------------
// Unix
// ---------------------------------------------------------------------------

#[cfg(unix)]
mod unix {
    use super::*;
    use crate::transport::UnixWorkerTransport;
    use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;

    const F_GETFD: i32 = 1;
    const F_SETFD: i32 = 2;
    const FD_CLOEXEC: i32 = 1;

    extern "C" {
        fn fcntl(fd: i32, cmd: i32, arg: i32) -> i32;
    }

    fn clear_cloexec(fd: RawFd) -> io::Result<()> {
        // SAFETY: `fd` is a live descriptor from our `File`/`UnixStream`.
        // F_GETFD/F_SETFD only touch the close-on-exec flag.
        unsafe {
            let flags = fcntl(fd, F_GETFD, 0);
            if flags < 0 {
                return Err(io::Error::last_os_error());
            }
            if fcntl(fd, F_SETFD, flags & !FD_CLOEXEC) < 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }

    pub fn spawn_worker(
        worker_exe: &Path,
        doc: Option<&File>,
        extra_env: &[(&str, &str)],
    ) -> io::Result<WorkerChild> {
        let seq = SPAWN_SEQ.fetch_add(1, Ordering::Relaxed);
        let sock_path: PathBuf = std::env::temp_dir().join(format!(
            "pdf-platform-ipc-{}-{}.sock",
            std::process::id(),
            seq
        ));
        let _ = std::fs::remove_file(&sock_path);

        let listener = UnixListener::bind(&sock_path)?;
        struct Unlink(PathBuf);
        impl Drop for Unlink {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.0);
            }
        }
        let unlink = Unlink(sock_path.clone());

        let mut cmd = Command::new(worker_exe);
        cmd.env(ENV_IPC_SOCK, &sock_path)
            .env_remove(ENV_IPC_PORT)
            .env_remove(ENV_DOC_HANDLE)
            .env_remove("PDF_PLATFORM_DOC_PATH");
        apply_extra_env(&mut cmd, extra_env);

        if let Some(file) = doc {
            let fd = file.as_raw_fd();
            clear_cloexec(fd)?;
            cmd.env(ENV_DOC_FD, fd.to_string());
        } else {
            cmd.env_remove(ENV_DOC_FD);
        }

        let child = cmd.spawn().map_err(|e| {
            io::Error::new(e.kind(), format!("spawn {}: {e}", worker_exe.display()))
        })?;

        listener.set_nonblocking(false)?;
        let (stream, _) = listener
            .accept()
            .map_err(|e| io::Error::new(e.kind(), format!("accept worker IPC: {e}")))?;
        drop(unlink);

        let transport = Box::new(UnixWorkerTransport::from_stream(stream));
        Ok(WorkerChild { transport, child })
    }

    pub fn adopt_inherited() -> io::Result<Box<dyn protocol::transport::WorkerTransport>> {
        let path = std::env::var(ENV_IPC_SOCK).map_err(|_| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("missing env {ENV_IPC_SOCK}"),
            )
        })?;
        let stream = connect_with_retry(|| UnixStream::connect(&path), 50, 100)?;
        Ok(Box::new(UnixWorkerTransport::from_stream(stream)))
    }

    pub fn adopt_document_file() -> io::Result<Option<File>> {
        let Ok(raw) = std::env::var(ENV_DOC_FD) else {
            return Ok(None);
        };
        let fd: RawFd = raw.parse().map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("bad {ENV_DOC_FD}: {raw}"),
            )
        })?;
        // SAFETY: parent set ENV_DOC_FD to an inheritable FD dedicated to this
        // worker process; we take ownership exactly once.
        let file = unsafe { File::from_raw_fd(fd) };
        Ok(Some(file))
    }
}

// ---------------------------------------------------------------------------
// Windows
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod windows {
    use super::*;
    use crate::transport::WindowsWorkerTransport;
    use std::net::{TcpListener, TcpStream};
    use std::os::windows::io::{AsRawHandle, FromRawHandle, RawHandle};

    const HANDLE_FLAG_INHERIT: u32 = 0x0000_0001;

    #[link(name = "kernel32")]
    extern "system" {
        fn SetHandleInformation(handle: *mut core::ffi::c_void, mask: u32, flags: u32) -> i32;
    }

    fn set_inheritable(handle: RawHandle) -> io::Result<()> {
        // SAFETY: handle is a live Windows HANDLE from our File; SetHandleInformation
        // only toggles the inherit flag (HANDLE_FLAG_INHERIT).
        let ok = unsafe {
            SetHandleInformation(handle as *mut core::ffi::c_void, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT)
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub fn spawn_worker(
        worker_exe: &Path,
        doc: Option<&File>,
        extra_env: &[(&str, &str)],
    ) -> io::Result<WorkerChild> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();

        let mut cmd = Command::new(worker_exe);
        cmd.env(ENV_IPC_PORT, port.to_string())
            .env_remove(ENV_IPC_SOCK)
            .env_remove(ENV_DOC_FD)
            .env_remove("PDF_PLATFORM_DOC_PATH");
        apply_extra_env(&mut cmd, extra_env);

        if let Some(file) = doc {
            let handle = file.as_raw_handle();
            set_inheritable(handle)?;
            let as_int = handle as usize;
            cmd.env(ENV_DOC_HANDLE, as_int.to_string());
        } else {
            cmd.env_remove(ENV_DOC_HANDLE);
        }

        let child = cmd.spawn().map_err(|e| {
            io::Error::new(e.kind(), format!("spawn {}: {e}", worker_exe.display()))
        })?;

        let (stream, _) = listener
            .accept()
            .map_err(|e| io::Error::new(e.kind(), format!("accept worker IPC: {e}")))?;
        let transport = Box::new(WindowsWorkerTransport::from_stream(stream)?);
        Ok(WorkerChild { transport, child })
    }

    pub fn adopt_inherited() -> io::Result<Box<dyn protocol::transport::WorkerTransport>> {
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

    pub fn adopt_document_file() -> io::Result<Option<File>> {
        let Ok(raw) = std::env::var(ENV_DOC_HANDLE) else {
            return Ok(None);
        };
        let as_int: usize = raw.parse().map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("bad {ENV_DOC_HANDLE}: {raw}"),
            )
        })?;
        let handle = as_int as RawHandle;
        // SAFETY: parent set ENV_DOC_HANDLE to an inheritable HANDLE value for
        // this child only; we take ownership exactly once.
        let file = unsafe { File::from_raw_handle(handle) };
        Ok(Some(file))
    }
}
