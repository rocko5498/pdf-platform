//! OS sandbox profiles: seccomp-bpf (Linux), AppContainer+job (Windows), Sandbox (macOS). [ADR-008, ADR-016]
//!
//! SECURITY: human-gated — draft only, do not weaken any filter. [IG AI-6]
//!
//! Lockdown ordering: sandbox is applied BEFORE any document handle or
//! untrusted input enters the process. [SDS §3.1 step 3]
//!
//! ## Status: ADVISORY (default)
//!
//! The implementation establishes the correct API, call sequence, and a
//! machine-readable report of the *intended* profile. It does **not** apply
//! irreversible OS filters until a human security review accepts
//! `docs/security/confinement-review-package.md` and enforcement is enabled
//! via an explicit feature (not yet on by default).
//!
//! Never claim "sandboxed" while [`ConfinementMode::Advisory`].

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

/// Whether OS filters are actually applied. [ADR-016]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfinementMode {
    /// Log intended profile; do not fail closed. Default until human review.
    Advisory,
    /// Apply filters; failure is fatal. Requires review + feature flag.
    Enforced,
}

/// Current process-wide mode. Always Advisory unless a reviewed build enables
/// enforcement (no default path to Enforced without human sign-off).
pub fn current_mode() -> ConfinementMode {
    // Explicitly not reading an env override that would silently enable
    // unreviewed filters. Enforcement is a future feature flag after review.
    ConfinementMode::Advisory
}

/// Documented Linux syscall allowlist (intent). [ADR-016, SDS §12.2]
/// Security review must approve before BPF generation.
pub const LINUX_SYSCALL_ALLOWLIST: &[&str] = &[
    "read",
    "write",
    "mmap",
    "mprotect",
    "munmap",
    "mremap",
    "brk",
    "madvise",
    "futex",
    "clone",
    "exit",
    "exit_group",
    "rt_sigaction",
    "rt_sigprocmask",
    "sched_yield",
    "close",
    "fstat",
    "lseek",
    "getrandom",
    "clock_gettime",
];

/// Documented Linux deny list (intent).
pub const LINUX_SYSCALL_DENYLIST: &[&str] = &[
    "socket",
    "connect",
    "bind",
    "listen",
    "accept",
    "execve",
    "fork",
    "vfork",
    "ptrace",
    "process_vm_readv",
    "process_vm_writev",
];

/// Machine-readable confinement status for diagnostics. [FR-DIAG, ADR-020]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfinementReport {
    /// Operating system target.
    pub platform: &'static str,
    /// Advisory vs enforced.
    pub mode: ConfinementMode,
    /// Human-readable summary lines (what would be / is applied).
    pub profile_lines: Vec<String>,
    /// True only when OS filters are actually installed.
    pub filters_active: bool,
}

impl ConfinementReport {
    /// Format for CLI / diagnostics panel.
    pub fn display_text(&self) -> String {
        let mut out = format!(
            "platform={}\nmode={:?}\nfilters_active={}\n",
            self.platform, self.mode, self.filters_active
        );
        for line in &self.profile_lines {
            out.push_str("  ");
            out.push_str(line);
            out.push('\n');
        }
        if self.mode == ConfinementMode::Advisory {
            out.push_str(
                "NOTE: Advisory only — see docs/security/confinement-review-package.md\n",
            );
        }
        out
    }
}

/// Build a report of the intended profile for this OS (no side effects).
pub fn confinement_report() -> ConfinementReport {
    let mode = current_mode();
    #[cfg(target_os = "linux")]
    {
        let mut lines: Vec<String> = vec![
            "user namespace (map to nobody)".into(),
            "mount namespace (private)".into(),
            "network: none".into(),
            format!("seccomp allow: {}", LINUX_SYSCALL_ALLOWLIST.join(", ")),
            format!("seccomp deny: {}", LINUX_SYSCALL_DENYLIST.join(", ")),
            "seccomp default: KILL (fail-closed after review)".into(),
        ];
        if mode == ConfinementMode::Advisory {
            lines.insert(0, "STATUS: would apply (not yet enforced)".into());
        }
        return ConfinementReport {
            platform: "linux",
            mode,
            profile_lines: lines,
            filters_active: false, // never true until Enforced ships
        };
    }
    #[cfg(target_os = "windows")]
    {
        return ConfinementReport {
            platform: "windows",
            mode,
            profile_lines: vec![
                "STATUS: would apply (not yet enforced)".into(),
                "AppContainer profile (restricted capabilities)".into(),
                "Job object: KILL_ON_JOB_CLOSE, memory + CPU caps".into(),
                "Allowed handles: IPC pipe, document file, shmem".into(),
            ],
            filters_active: false,
        };
    }
    #[cfg(target_os = "macos")]
    {
        return ConfinementReport {
            platform: "macos",
            mode,
            profile_lines: vec![
                "STATUS: would apply (not yet enforced)".into(),
                "sandbox_init: deny network + filesystem except inherited".into(),
                "allow: mach IPC, memory management".into(),
            ],
            filters_active: false,
        };
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        ConfinementReport {
            platform: "unknown",
            mode,
            profile_lines: vec!["no confinement implementation".into()],
            filters_active: false,
        }
    }
}

/// Apply OS-specific sandbox confinement to the current process (child side).
///
/// Called at the start of worker-main `main()`, before adopting any handles.
pub fn lockdown_worker() -> Result<(), ConfinementError> {
    let report = confinement_report();
    for line in &report.profile_lines {
        eprintln!("worker: [ADVISORY] {line}");
    }
    eprintln!(
        "worker: confinement mode={:?} filters_active={}",
        report.mode, report.filters_active
    );

    // Enforcement path deliberately absent until review package is signed.
    // Do not add silent env-based enablement of real filters here.
    match report.mode {
        ConfinementMode::Advisory => Ok(()),
        ConfinementMode::Enforced => {
            // Placeholder: real filters would apply here post-review.
            Err(ConfinementError::Unsupported(
                "Enforced mode not enabled in this build — complete security review first".into(),
            ))
        }
    }
}

/// Apply OS-specific restrictions to a child process from the parent side.
pub fn confine_child(_child: &std::process::Child) -> Result<(), ConfinementError> {
    #[cfg(target_os = "windows")]
    {
        let report = confinement_report();
        for line in &report.profile_lines {
            eprintln!("parent: [ADVISORY] {line}");
        }
        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lockdown_worker_runs_advisory() {
        let result = lockdown_worker();
        assert!(result.is_ok(), "advisory lockdown should succeed: {result:?}");
        assert_eq!(current_mode(), ConfinementMode::Advisory);
    }

    #[test]
    fn report_never_claims_filters_active_while_advisory() {
        let r = confinement_report();
        assert_eq!(r.mode, ConfinementMode::Advisory);
        assert!(
            !r.filters_active,
            "must not claim active filters before review"
        );
        let text = r.display_text();
        assert!(text.contains("Advisory") || text.contains("ADVISORY") || text.contains("mode="));
        assert!(!r.profile_lines.is_empty());
    }

    #[test]
    fn linux_allowlist_has_no_network_syscalls() {
        for s in LINUX_SYSCALL_ALLOWLIST {
            assert!(
                !matches!(*s, "socket" | "connect" | "bind" | "listen" | "accept"),
                "network syscall {s} must not be on allowlist"
            );
        }
        assert!(LINUX_SYSCALL_DENYLIST.contains(&"socket"));
        assert!(LINUX_SYSCALL_DENYLIST.contains(&"execve"));
    }

    #[test]
    fn confine_child_noop_on_non_windows() {
        #[cfg(not(target_os = "windows"))]
        {
            let child = std::process::Command::new("echo").spawn().unwrap();
            let result = confine_child(&child);
            assert!(result.is_ok());
            let _ = child.wait();
        }
    }
}
