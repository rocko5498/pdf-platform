//! Worker process spawn + IPC + document/shmem handle inherit. [ADR-008, SDS §3.1, §4.2, §10.1]
//!
//! **Call order (M0):** create IPC listen end → mark handles inheritable →
//! spawn worker → accept IPC → return parent transport.
//!
//! ## Env contract (child)
//!
//! | Variable | Meaning |
//! |----------|---------|
//! | `PDF_PLATFORM_IPC_SOCK` | Unix: path to parent `UnixListener` |
//! | `PDF_PLATFORM_IPC_PORT` | Windows: `127.0.0.1` TCP port |
//! | `PDF_PLATFORM_DOC_FD` / `_HANDLE` | Inherited document file |
//! | `PDF_PLATFORM_SHMEM_FD` / `_HANDLE` | Inherited tile shared-memory file |
//!
//! Paths are **not** passed for document/shmem (GR-1).

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
/// Unix: inherited shared-memory FD.
pub const ENV_SHMEM_FD: &str = "PDF_PLATFORM_SHMEM_FD";
/// Windows: inherited shared-memory HANDLE as decimal integer.
pub const ENV_SHMEM_HANDLE: &str = "PDF_PLATFORM_SHMEM_HANDLE";

static SPAWN_SEQ: AtomicU64 = AtomicU64::new(1);

/// Optional files to inherit into the worker.
#[derive(Default)]
pub struct SpawnAttachments<'a> {
    /// Brokered document file.
    pub doc: Option<&'a File>,
    /// Shared tile buffer file.
    pub shmem: Option<&'a File>,
}

/// Parent-side handle: framed IPC + child process. [SDS §10.1]
pub struct WorkerChild {
    /// Control channel to the worker.
    pub transport: Box<dyn protocol::transport::WorkerTransport>,
    /// OS process; drop does not kill — call `kill` or `wait` explicitly.
    pub child: Child,
}

/// Spawn `worker_exe` and establish a framed control channel (no attachments).
pub fn spawn_worker(worker_exe: &Path) -> io::Result<WorkerChild> {
    spawn_worker_with_env(worker_exe, &[])
}

/// Like [`spawn_worker`], with extra environment variables for the child.
pub fn spawn_worker_with_env(
    worker_exe: &Path,
    extra_env: &[(&str, &str)],
) -> io::Result<WorkerChild> {
    spawn_impl(worker_exe, &SpawnAttachments::default(), extra_env)
}

/// Spawn worker with an inheritable document file (no path string). [SDS §3.1]
pub fn spawn_worker_with_file(
    worker_exe: &Path,
    doc: &File,
    extra_env: &[(&str, &str)],
) -> io::Result<WorkerChild> {
    spawn_impl(
        worker_exe,
        &SpawnAttachments {
            doc: Some(doc),
            shmem: None,
        },
        extra_env,
    )
}

/// Spawn worker with document and/or shmem attachments.
pub fn spawn_worker_with_attachments(
    worker_exe: &Path,
    attachments: &SpawnAttachments<'_>,
    extra_env: &[(&str, &str)],
) -> io::Result<WorkerChild> {
    spawn_impl(worker_exe, attachments, extra_env)
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
        return unix::adopt_file_from_env(ENV_DOC_FD);
    }
    #[cfg(windows)]
    {
        return windows::adopt_file_from_env(ENV_DOC_HANDLE);
    }
    #[cfg(not(any(unix, windows)))]
    {
        Ok(None)
    }
}

/// Adopt the inherited shared-memory file, if the parent attached one.
pub fn adopt_shmem_file() -> io::Result<Option<File>> {
    #[cfg(unix)]
    {
        return unix::adopt_file_from_env(ENV_SHMEM_FD);
    }
    #[cfg(windows)]
    {
        return windows::adopt_file_from_env(ENV_SHMEM_HANDLE);
    }
    #[cfg(not(any(unix, windows)))]
    {
        Ok(None)
    }
}

fn spawn_impl(
    worker_exe: &Path,
    attachments: &SpawnAttachments<'_>,
    extra_env: &[(&str, &str)],
) -> io::Result<WorkerChild> {
    #[cfg(unix)]
    {
        return unix::spawn_worker(worker_exe, attachments, extra_env);
    }
    #[cfg(windows)]
    {
        return windows::spawn_worker(worker_exe, attachments, extra_env);
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (worker_exe, attachments, extra_env);
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
        // SAFETY: live FD from our File; only toggles close-on-exec.
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

    fn set_inherited_fd(cmd: &mut Command, env_key: &str, file: &File) -> io::Result<()> {
        let fd = file.as_raw_fd();
        clear_cloexec(fd)?;
        cmd.env(env_key, fd.to_string());
        Ok(())
    }

    pub fn spawn_worker(
        worker_exe: &Path,
        attachments: &SpawnAttachments<'_>,
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
            .env_remove(ENV_SHMEM_HANDLE)
            .env_remove("PDF_PLATFORM_DOC_PATH");
        apply_extra_env(&mut cmd, extra_env);

        if let Some(file) = attachments.doc {
            set_inherited_fd(&mut cmd, ENV_DOC_FD, file)?;
        } else {
            cmd.env_remove(ENV_DOC_FD);
        }
        if let Some(file) = attachments.shmem {
            set_inherited_fd(&mut cmd, ENV_SHMEM_FD, file)?;
        } else {
            cmd.env_remove(ENV_SHMEM_FD);
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

    pub fn adopt_file_from_env(env_key: &str) -> io::Result<Option<File>> {
        let Ok(raw) = std::env::var(env_key) else {
            return Ok(None);
        };
        let fd: RawFd = raw.parse().map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("bad {env_key}: {raw}"),
            )
        })?;
        // SAFETY: parent set env to an inheritable FD for this process only.
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
        // SAFETY: live HANDLE from our File; only toggles inherit flag.
        let ok = unsafe {
            SetHandleInformation(
                handle as *mut core::ffi::c_void,
                HANDLE_FLAG_INHERIT,
                HANDLE_FLAG_INHERIT,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn set_inherited_handle(cmd: &mut Command, env_key: &str, file: &File) -> io::Result<()> {
        let handle = file.as_raw_handle();
        set_inheritable(handle)?;
        cmd.env(env_key, (handle as usize).to_string());
        Ok(())
    }

    pub fn spawn_worker(
        worker_exe: &Path,
        attachments: &SpawnAttachments<'_>,
        extra_env: &[(&str, &str)],
    ) -> io::Result<WorkerChild> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();

        let mut cmd = Command::new(worker_exe);
        cmd.env(ENV_IPC_PORT, port.to_string())
            .env_remove(ENV_IPC_SOCK)
            .env_remove(ENV_DOC_FD)
            .env_remove(ENV_SHMEM_FD)
            .env_remove("PDF_PLATFORM_DOC_PATH");
        apply_extra_env(&mut cmd, extra_env);

        if let Some(file) = attachments.doc {
            set_inherited_handle(&mut cmd, ENV_DOC_HANDLE, file)?;
        } else {
            cmd.env_remove(ENV_DOC_HANDLE);
        }
        if let Some(file) = attachments.shmem {
            set_inherited_handle(&mut cmd, ENV_SHMEM_HANDLE, file)?;
        } else {
            cmd.env_remove(ENV_SHMEM_HANDLE);
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

    pub fn adopt_file_from_env(env_key: &str) -> io::Result<Option<File>> {
        let Ok(raw) = std::env::var(env_key) else {
            return Ok(None);
        };
        let as_int: usize = raw.parse().map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("bad {env_key}: {raw}"),
            )
        })?;
        let handle = as_int as RawHandle;
        // SAFETY: parent set env to an inheritable HANDLE for this process only.
        let file = unsafe { File::from_raw_handle(handle) };
        Ok(Some(file))
    }
}
