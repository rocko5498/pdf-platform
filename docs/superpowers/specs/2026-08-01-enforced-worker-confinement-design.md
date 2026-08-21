# Enforced Worker Confinement Design

**Date:** 2026-08-01
**Milestone:** M0 — foundations and walking skeleton
**Status:** Approved implementation direction; security-critical drafts remain human-gated
**Supersedes:** `docs/superpowers/specs/2026-07-13-confinement-design.md`
**Citations:** ADR-008, ADR-016, ADR-020, ADR-022, SDS §3.1, SDS §12.2, SDS §14 M0, IG AI-4..6, IG CR-Z, GR-1, GR-7, GR-8

## Problem

The M0 exit criterion requires document workers to run sandboxed on Windows,
Linux, and macOS. The current `sandbox::confinement` implementation is
deliberately advisory: it logs an intended profile, applies no OS restriction,
and reports `filters_active=false`. The milestone tracker therefore cannot
honestly claim enforced confinement.

The earlier design mixed a production target with advisory non-goals and left
the enforcement boundary as a placeholder. This design separates the shared,
safe policy contract from three independently reviewable OS implementations.

## Decision

Implement confinement in four reviewable slices:

1. A shared, safe enforcement contract and negative-test protocol.
2. Linux namespaces plus seccomp-bpf.
3. Windows AppContainer launch plus a job object.
4. A macOS `sandbox_init` profile.

All enforcement is compile-time selected and default-off until the human
security review in `docs/security/confinement-review-package.md` is signed.
There is no environment-variable or runtime switch that can silently enable an
unreviewed profile.

Each OS slice may land as a clearly marked draft while the default remains
advisory. M0 is not marked complete until all three OS jobs prove both denial
and legitimate worker operation with enforcement enabled.

## Shared contract

### Build policy

The `sandbox` crate exposes one aggregate Cargo feature:

```toml
[features]
default = []
enforced-confinement = []
```

Without the feature, only `Advisory` is available. With the feature, worker
startup requests enforcement from the platform backend. A missing backend or
failed installation terminates worker startup; it never falls back to
advisory mode.

### State model

Replace the current compile-time-looking report with explicit lifecycle state:

```rust
pub enum ConfinementState {
    Advisory,
    EnforcementPending,
    Enforced,
}

pub struct ConfinementReport {
    pub platform: &'static str,
    pub state: ConfinementState,
    pub profile_lines: Vec<String>,
    pub filters_active: bool,
}
```

Required invariants:

- `filters_active` is true only with `state == Enforced`.
- `Advisory` and `EnforcementPending` never use the word “sandboxed.”
- Installation failures are typed `ConfinementError` values and cause worker
  startup failure; they are never converted into a successful report.
- A report created before installation is never reused as proof that filters
  became active.

### Backend seam

The shared module owns policy and reporting. A private platform module owns OS
calls:

```rust
trait PlatformConfinement {
    fn install(&self) -> Result<Vec<String>, ConfinementError>;
}

pub struct ActiveConfinement {
    report: ConfinementReport,
}
```

`ActiveConfinement` can only be constructed by the platform backend after all
required restrictions are installed. The shared module validates the report
invariants before returning it to `worker-main`.

For the safe contract slice, the private backend is injected in unit tests.
Production builds with `enforced-confinement` fail closed with
`ConfinementError::BackendUnavailable` until their OS slice supplies an
implementation. This is intentional and prevents scaffolding from becoming a
false security claim.

## Platform slices

### Linux

The child installs confinement before adopting document, shared-memory, or IPC
handles:

1. Set `PR_SET_NO_NEW_PRIVS`.
2. Create user, mount, PID, and network namespaces; an unavailable or rejected
   namespace is fatal in an enforced build.
3. Make the mount namespace private.
4. Install a reviewed seccomp-bpf allowlist with a kill-by-default action.
5. Return `ActiveConfinement` only after the kernel accepts the filter.

The allowlist must support the existing PDFium/Skia render, text extraction,
threading, inherited-handle, shared-memory, and respawn tests. Network,
arbitrary path open, process creation, ptrace, cross-process memory access, and
new privilege acquisition remain denied.

### Windows

AppContainer is a launch property, so the existing `confine_child(&Child)` API
is removed rather than pretending a post-spawn call can create the required
boundary.

The parent constructs a suspended worker with
`PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES`, an AppContainer SID with no
network capability, and only explicitly inherited IPC/document/shared-memory
handles. It assigns the process to a job object with
`KILL_ON_JOB_CLOSE` and reviewed memory/CPU bounds before resuming the primary
thread. A failure at any step terminates the suspended process and returns a
typed `ConfinementError`.

### macOS

The child calls `sandbox_init` before adopting handles. The reviewed profile:

- denies network access and process creation;
- denies arbitrary user path access;
- permits inherited descriptors, required Mach services, memory management,
  threading, and the minimum read-only runtime image access required to load
  the pinned PDFium library;
- returns `ActiveConfinement` only after `sandbox_init` succeeds.

## Data flow and startup ordering

```text
Z0 prepares IPC + brokered handles
  -> platform launch preparation (Windows only)
  -> worker process starts
  -> lockdown_worker()
  -> platform confirms active restrictions
  -> worker adopts inherited handles
  -> worker initializes PDFium
  -> worker accepts untrusted document requests
```

No document bytes, path, password, or plugin code may be interpreted before
the active-restriction confirmation in an enforced build.

Dynamic loading of the pinned PDFium runtime is a platform-profile exception,
not general filesystem authority. Each OS denial test must prove that the
worker still cannot open a user-selected path after engine initialization.

## Error handling and diagnostics

- Advisory builds keep the current truthful advisory diagnostic.
- Enforced builds exit non-zero on unsupported kernels, unavailable APIs,
  profile rejection, partial installation, or invariant failure.
- The coordinator surfaces worker startup failure as a typed diagnostic; it
  does not respawn indefinitely.
- A denial-test probe reports only the denied capability and OS error category;
  it never includes host paths or secrets.
- Job-object limits and any bounded test buffers are explicit to satisfy GR-7.

## Test strategy

### Shared contract tests

- Default build reports `Advisory` and `filters_active=false`.
- Enforced request with no backend returns `BackendUnavailable`.
- A backend failure never degrades to advisory success.
- Invalid combinations such as `Advisory + filters_active=true` are rejected.
- Only a successful backend can produce `Enforced + filters_active=true`.

### Per-OS denial tests

Each enforced CI job runs a dedicated worker probe and proves:

- loopback and external socket creation/connect are denied;
- arbitrary path open/read/write are denied;
- child-process creation is denied;
- peer-process inspection is denied where the OS exposes the primitive.

### Legitimate-operation regression tests

The same enforced job must also pass:

- inherited IPC ping/quit;
- document-handle inspection;
- shared-memory tile smoke;
- real PDFium page rendering;
- text extraction;
- kill-worker detection and transparent respawn.

Tests run in both advisory and enforced configurations. A platform cannot be
called complete if only the negative probes pass or only normal rendering
passes.

## Dependency and unsafe policy

The shared contract slice adds no dependency and no `unsafe`.

Each OS slice requires a separate dependency/unsafe review before code is
written:

- direct platform dependencies must be exact-version pinned, license-checked,
  and contained behind the private backend seam;
- all OS calls remain in the designated `sandbox` modules;
- every `unsafe` block carries a `// SAFETY:` proof;
- CR-Z requires two reviewers, including a sandbox/unsafe owner.

This design does not pre-approve a platform dependency or an unsafe block.

## Delivery and milestone truth

Draft platform slices may be committed and tested with the aggregate feature
remaining default-off. Promotion requires:

1. Signed confinement review package.
2. Two-reviewer approval for the security-critical and unsafe surfaces.
3. Enforced Windows, Linux, and macOS CI denial tests.
4. Enforced legitimate-operation regression tests on all three OSes.
5. Milestone tracker updated from `Advisory` to `Met` only after evidence exists.

Until all five conditions hold, public diagnostics and project status continue
to say “Advisory confinement,” never “sandboxed.”

## Non-goals

- Changing the ADR-mandated OS mechanisms.
- Adding a runtime bypass or best-effort fallback.
- Expanding broker capabilities.
- Confining Z0 or moving document parsing into Z0.
- Declaring M0 complete from unit tests or one operating system.
