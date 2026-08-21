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
//! Never claim "sandboxed" while [`ConfinementState::Advisory`].

use std::fmt;

/// Errors from confinement operations.
#[derive(Debug)]
pub enum ConfinementError {
    /// Platform-specific error message.
    Platform(String),
    /// The confinement mechanism is not available on this platform.
    Unsupported(String),
    /// This build requires enforcement but no reviewed backend is present.
    BackendUnavailable(&'static str),
    /// A report combined state and active-filter evidence inconsistently.
    InvalidReport(String),
}

impl fmt::Display for ConfinementError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Platform(msg) => write!(f, "confinement error: {msg}"),
            Self::Unsupported(msg) => write!(f, "confinement unsupported: {msg}"),
            Self::BackendUnavailable(platform) => {
                write!(f, "enforced confinement backend unavailable for {platform}")
            }
            Self::InvalidReport(msg) => write!(f, "invalid confinement report: {msg}"),
        }
    }
}

impl std::error::Error for ConfinementError {}

/// Whether OS filters are actually applied. [ADR-016]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfinementState {
    /// Log intended profile; do not fail closed. Default until human review.
    Advisory,
    /// This build requires a reviewed OS backend before worker startup.
    EnforcementPending,
    /// Apply filters; failure is fatal. Requires review + feature flag.
    Enforced,
}

/// Confinement state requested by this build; no runtime override exists.
pub const fn requested_state() -> ConfinementState {
    #[cfg(feature = "enforced-confinement")]
    {
        ConfinementState::EnforcementPending
    }
    #[cfg(not(feature = "enforced-confinement"))]
    {
        ConfinementState::Advisory
    }
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
    /// Advisory, pending installation, or proven enforced.
    pub state: ConfinementState,
    /// Human-readable summary lines (what would be / is applied).
    pub profile_lines: Vec<String>,
    /// True only when OS filters are actually installed.
    pub filters_active: bool,
}

impl ConfinementReport {
    /// Reject any report that claims active filters without proven enforcement.
    pub fn validate(&self) -> Result<(), ConfinementError> {
        if self.filters_active != (self.state == ConfinementState::Enforced) {
            return Err(ConfinementError::InvalidReport(format!(
                "state={:?} filters_active={}",
                self.state, self.filters_active
            )));
        }
        Ok(())
    }

    /// Format for CLI / diagnostics panel.
    pub fn display_text(&self) -> String {
        let mut out = format!(
            "platform={}\nstate={:?}\nfilters_active={}\n",
            self.platform, self.state, self.filters_active
        );
        for line in &self.profile_lines {
            out.push_str("  ");
            out.push_str(line);
            out.push('\n');
        }
        match self.state {
            ConfinementState::Advisory => out.push_str(
                "NOTE: Advisory only — see docs/security/confinement-review-package.md\n",
            ),
            ConfinementState::EnforcementPending => {
                out.push_str("NOTE: Enforcement requested; OS filters are not active yet\n")
            }
            ConfinementState::Enforced => {}
        }
        out
    }
}

trait PlatformConfinement {
    fn install(&self) -> Result<Vec<String>, ConfinementError>;
}

struct UnavailableBackend {
    platform: &'static str,
}

impl PlatformConfinement for UnavailableBackend {
    fn install(&self) -> Result<Vec<String>, ConfinementError> {
        Err(ConfinementError::BackendUnavailable(self.platform))
    }
}

/// Proof that the selected platform backend installed all required filters.
#[derive(Debug)]
pub struct ActiveConfinement {
    report: ConfinementReport,
}

impl ActiveConfinement {
    /// Return the validated active-filter report.
    pub fn report(&self) -> &ConfinementReport {
        &self.report
    }

    /// Consume the proof and return its validated report.
    pub fn into_report(self) -> ConfinementReport {
        self.report
    }
}

fn install_with<B: PlatformConfinement>(
    backend: &B,
) -> Result<ActiveConfinement, ConfinementError> {
    let report = ConfinementReport {
        platform: std::env::consts::OS,
        state: ConfinementState::Enforced,
        profile_lines: backend.install()?,
        filters_active: true,
    };
    report.validate()?;
    Ok(ActiveConfinement { report })
}

fn preinstall_status(state: ConfinementState) -> &'static str {
    match state {
        ConfinementState::Advisory => "STATUS: would apply (not yet enforced)",
        ConfinementState::EnforcementPending => "STATUS: enforcement pending platform installation",
        ConfinementState::Enforced => "STATUS: active",
    }
}

/// Build a report of the intended profile for this OS (no side effects).
pub fn confinement_report() -> ConfinementReport {
    let state = requested_state();
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
        lines.insert(0, preinstall_status(state).into());
        return ConfinementReport {
            platform: "linux",
            state,
            profile_lines: lines,
            filters_active: false, // never true until Enforced ships
        };
    }
    #[cfg(target_os = "windows")]
    {
        return ConfinementReport {
            platform: "windows",
            state,
            profile_lines: vec![
                preinstall_status(state).into(),
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
            state,
            profile_lines: vec![
                preinstall_status(state).into(),
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
            state,
            profile_lines: vec!["no confinement implementation".into()],
            filters_active: false,
        }
    }
}

/// Apply OS-specific sandbox confinement to the current process (child side).
///
/// Called at the start of worker-main `main()`, before adopting any handles.
pub fn lockdown_worker() -> Result<ConfinementReport, ConfinementError> {
    let report = confinement_report();
    log_report("worker", &report);

    match report.state {
        ConfinementState::Advisory => Ok(report),
        ConfinementState::EnforcementPending => install_with(&UnavailableBackend {
            platform: report.platform,
        })
        .map(ActiveConfinement::into_report),
        ConfinementState::Enforced => Err(ConfinementError::InvalidReport(
            "pre-install report cannot already be enforced".into(),
        )),
    }
}

fn log_report(prefix: &str, report: &ConfinementReport) {
    let label = match report.state {
        ConfinementState::Advisory => "ADVISORY",
        ConfinementState::EnforcementPending => "PENDING",
        ConfinementState::Enforced => "ENFORCED",
    };
    for line in &report.profile_lines {
        eprintln!("{prefix}: [{label}] {line}");
    }
    eprintln!(
        "{prefix}: confinement state={:?} filters_active={}",
        report.state, report.filters_active
    );
}

/// Apply OS-specific restrictions to a child process from the parent side.
pub fn confine_child(_child: &std::process::Child) -> Result<(), ConfinementError> {
    #[cfg(target_os = "windows")]
    {
        let report = confinement_report();
        log_report("parent", &report);
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

    struct SuccessfulBackend;

    impl PlatformConfinement for SuccessfulBackend {
        fn install(&self) -> Result<Vec<String>, ConfinementError> {
            Ok(vec!["test filters installed".into()])
        }
    }

    struct FailingBackend;

    impl PlatformConfinement for FailingBackend {
        fn install(&self) -> Result<Vec<String>, ConfinementError> {
            Err(ConfinementError::Platform("test rejection".into()))
        }
    }

    #[test]
    fn successful_backend_is_the_only_path_to_active_report() {
        let active = install_with(&SuccessfulBackend).expect("backend succeeds");
        let report = active.report();
        assert_eq!(report.state, ConfinementState::Enforced);
        assert!(report.filters_active);
        report.validate().expect("active report is valid");
    }

    #[test]
    fn backend_failure_is_not_downgraded_to_advisory() {
        let error = install_with(&FailingBackend).expect_err("failure must propagate");
        assert!(matches!(error, ConfinementError::Platform(_)));
    }

    #[cfg(feature = "enforced-confinement")]
    #[test]
    fn enforced_build_without_platform_backend_fails_closed() {
        assert!(matches!(
            lockdown_worker(),
            Err(ConfinementError::BackendUnavailable(_))
        ));
    }

    #[test]
    fn requested_state_matches_compile_time_policy() {
        #[cfg(not(feature = "enforced-confinement"))]
        assert_eq!(requested_state(), ConfinementState::Advisory);

        #[cfg(feature = "enforced-confinement")]
        assert_eq!(requested_state(), ConfinementState::EnforcementPending);
    }

    #[cfg(not(feature = "enforced-confinement"))]
    #[test]
    fn advisory_startup_returns_inactive_report() {
        let report = lockdown_worker().expect("advisory startup succeeds");
        assert_eq!(report.state, ConfinementState::Advisory);
        assert!(!report.filters_active);
    }

    #[test]
    fn advisory_report_rejects_active_filters() {
        let report = ConfinementReport {
            platform: "test",
            state: ConfinementState::Advisory,
            profile_lines: vec!["test profile".into()],
            filters_active: true,
        };

        assert!(matches!(
            report.validate(),
            Err(ConfinementError::InvalidReport(_))
        ));
    }

    #[test]
    fn pending_report_is_inactive_and_never_claims_sandboxing() {
        let report = ConfinementReport {
            platform: "test",
            state: ConfinementState::EnforcementPending,
            profile_lines: vec!["test profile".into()],
            filters_active: false,
        };

        report
            .validate()
            .expect("pending report is internally valid");
        let text = report.display_text().to_ascii_lowercase();
        assert!(!text.contains("sandboxed"));
        assert!(text.contains("pending"));
    }

    #[test]
    fn platform_report_matches_requested_state_before_installation() {
        let report = confinement_report();
        assert_eq!(report.state, requested_state());
        assert!(!report.filters_active);
        report
            .validate()
            .expect("pre-install report must be honest");
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
            let mut child = std::process::Command::new("echo").spawn().unwrap();
            let result = confine_child(&child);
            assert!(result.is_ok());
            let _ = child.wait();
        }
    }
}
