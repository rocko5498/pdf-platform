//! OS sandbox profiles: seccomp-bpf (Linux), AppContainer+job (Windows), Sandbox (macOS). [ADR-008, ADR-016]
//!
//! SECURITY: human-gated — draft only, do not weaken any filter. [IG AI-6]
//!
//! This module establishes OS-level confinement for Z1 worker processes.
//! Lockdown ordering: sandbox is applied BEFORE any document handle or
//! untrusted input enters the process. [SDS §3.1 step 3]
//!
//! ## M0 status: ADVISORY
//!
//! The M0 implementation establishes the correct API and call sequence but
//! applies confinement as **advisory + logged**, not fatal. This means:
//! - The lockdown function runs and reports what it *would* do
//! - On failure, it logs a warning but does not kill the worker
//! - The actual BPF/AppContainer/sandbox profiles need security review before enforcement
//!
//! This matches ADR-016: "permission flags are honored by default but
//! documented as advisory." Never claim "sandboxed" until reviewed.

use std::fmt;

/// Errors from confinement operations.
#[derive(Debug)]
pub enum ConfinementError {
    /// Platform-specific error message.
    Platform(String),
    /// The confinement mechanism is not available on this platform.
    Unsupported(String),
}

impl fmt::Display for ConfinementError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Platform(msg) => write!(f, "confinement error: {msg}"),
            Self::Unsupported(msg) => write!(f, "confinement unsupported: {msg}"),
        }
    }
}

impl std::error::Error for ConfinementError {}

/// Apply OS-specific sandbox confinement to the current process (child side).
///
/// Called at the start of worker-main `main()`, before adopting any handles.
///
/// # M0 behavior
/// Logs what would be applied. Does not kill on failure.
///
/// # Safety
/// In production, this applies irreversible OS-level restrictions.
/// Must be called exactly once, before any untrusted input is processed.
pub fn lockdown_worker() -> Result<(), ConfinementError> {
    #[cfg(target_os = "linux")]
    {
        lockdown_linux()
    }

    #[cfg(target_os = "windows")]
    {
        lockdown_windows_child()
    }

    #[cfg(target_os = "macos")]
    {
        lockdown_macos()
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        Err(ConfinementError::Unsupported(
            "no confinement implementation for this platform".into(),
        ))
    }
}

/// Apply OS-specific restrictions to a child process from the parent side.
///
/// On Windows: creates AppContainer + job object (called before spawn).
/// On Linux/macOS: no-op (lockdown happens in the child).
pub fn confine_child(_child: &std::process::Child) -> Result<(), ConfinementError> {
    #[cfg(target_os = "windows")]
    {
        confine_child_windows(_child)
    }

    #[cfg(not(target_os = "windows"))]
    {
        // Linux/macOS: lockdown happens in the child, not from parent.
        Ok(())
    }
}

// ── Linux ───────────────────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
fn lockdown_linux() -> Result<(), ConfinementError> {
    // M0 DRAFT: log what would be applied. [IG AI-6]
    // Production: apply seccomp-bpf filter + namespaces here.

    eprintln!("worker: [ADVISORY] Linux confinement would apply:");
    eprintln!("  - user namespace (map to nobody)");
    eprintln!("  - mount namespace (private)");
    eprintln!("  - seccomp-bpf filter:");
    eprintln!("    ALLOW: read, write, mmap, mprotect, munmap, mremap, brk,");
    eprintln!("           madvise, futex, clone, exit, exit_group, rt_sigaction,");
    eprintln!("           rt_sigprocmask, sched_yield, close, fstat, lseek,");
    eprintln!("           getrandom, clock_gettime");
    eprintln!("    DENY:  socket, connect, bind, listen, accept, execve,");
    eprintln!("           fork, vfork, ptrace, process_vm_read/write");
    eprintln!("    KILL:  anything not in allow/deny (fail-closed)");

    // M0: advisory only — do not actually apply filters yet.
    // TODO: implement seccomp-bpf after security review.

    Ok(())
}

// ── Windows ─────────────────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
fn lockdown_windows_child() -> Result<(), ConfinementError> {
    // On Windows, primary confinement is parent-side (AppContainer + job).
    // The child-side lockdown is minimal.
    eprintln!("worker: [ADVISORY] Windows child-side lockdown (minimal — parent applies AppContainer)");
    Ok(())
}

#[cfg(target_os = "windows")]
fn confine_child_windows(_child: &std::process::Child) -> Result<(), ConfinementError> {
    // M0 DRAFT: log what would be applied. [IG AI-6]
    // Production: create AppContainer + job object here.

    eprintln!("worker: [ADVISORY] Windows parent-side confinement would apply:");
    eprintln!("  - AppContainer profile (restricted capabilities)");
    eprintln!("  - Job object:");
    eprintln!("    - KILL_ON_JOB_CLOSE");
    eprintln!("    - PROCESS_MEMORY limit");
    eprintln!("    - CPU_RATE hard cap");
    eprintln!("  - Allowed handles: IPC socket, document file, shmem file");

    // M0: advisory only — do not actually create AppContainer yet.
    // TODO: implement AppContainer after security review.

    Ok(())
}

// ── macOS ───────────────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
fn lockdown_macos() -> Result<(), ConfinementError> {
    // M0 DRAFT: log what would be applied. [IG AI-6]
    // Production: apply sandbox_init profile here.

    eprintln!("worker: [ADVISORY] macOS confinement would apply:");
    eprintln!("  - sandbox_init profile:");
    eprintln!("    DENY: network, filesystem (except inherited), process spawn");
    eprintln!("    ALLOW: mach IPC, memory management");

    // M0: advisory only — do not actually apply sandbox profile yet.
    // TODO: implement sandbox_init after security review.

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lockdown_worker_runs() {
        // M0: advisory lockdown should succeed without killing the process.
        let result = lockdown_worker();
        assert!(result.is_ok(), "advisory lockdown should succeed: {result:?}");
    }

    #[test]
    fn confine_child_noop_on_non_windows() {
        // On non-Windows, confine_child is a no-op.
        #[cfg(not(target_os = "windows"))]
        {
            let child = std::process::Command::new("echo").spawn().unwrap();
            let result = confine_child(&child);
            assert!(result.is_ok());
            let _ = child.wait();
        }
    }
}
