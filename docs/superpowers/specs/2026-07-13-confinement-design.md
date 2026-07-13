# Design: Worker Confinement (M0 draft — human-gated)

**Date:** 2026-07-13
**Milestone:** M0 walking skeleton
**Citations:** ADR-008, ADR-016, SDS §2.3, SDS §12.2, IG AI-6
**Status:** DRAFT — requires human review before any merge. Security-critical path.

## Problem

M0 exit criterion: "worker runs sandboxed." Currently `confinement.rs` is empty and no
lockdown is applied. Workers run fully unsandboxed with access to the entire OS.

## Architecture (from ADR-008/016, SDS §12.2)

| OS | Mechanism | Lockdown side |
|----|-----------|---------------|
| Linux | seccomp-bpf + namespaces | **Child** (worker-main main()) |
| Windows | AppContainer + job object | **Parent** (coordinator, before spawn) |
| macOS | sandbox_init profile | **Child** (worker-main main()) |

Critical ordering: sandbox established **before** document handle or guest code enters the process (SDS 3.1 step 3).

## Design

### 1. API (`confinement.rs`)

```rust
/// Apply OS-specific sandbox confinement to the current process (child side).
/// Called at the start of worker-main main(), before adopting any handles.
///
/// # Safety
/// This function applies irreversible OS-level restrictions. It must be called
/// exactly once, before any untrusted input is processed.
pub fn lockdown_worker() -> Result<(), ConfinementError>;

/// Apply OS-specific restrictions to a child process from the parent side.
/// Called on Windows before spawning the worker (AppContainer + job object).
/// On Linux/macOS this is a no-op (lockdown happens in the child).
pub fn confine_child(child: &Child) -> Result<(), ConfinementError>;
```

### 2. Linux (`cfg(target_os = "linux")`)

**Dependencies:** `libc` (already in tree), or `seccompiler` for BPF generation.

**Lockdown sequence (child-side):**
1. `unshare(CLONE_NEWUSER | CLONE_NEWNS | CLONE_NEWPID)` — user, mount, pid namespaces
2. Write UID/GID map (deny setgroups, map to nobody)
3. Load seccomp-bpf filter:
   - **ALLOW:** read, write, pread64, pwrite64, mmap, mprotect, munmap, mremap,
     brk, madvise, futex, clone (limited), exit, exit_group, rt_sigaction,
     rt_sigprocmask, sched_yield, close, fstat, lseek, ioctl (limited),
     getrandom, clock_gettime
   - **DENY (trap):** socket, connect, bind, listen, accept, sendto, recvfrom,
     execve, execveat, fork, vfork, clone3, mount, umount2, ptrace,
     process_vm_readv, process_vm_writev, keyctl, add_key, request_key
   - **KILL:** anything not in the allow/deny list (fail-closed)

**Namespace notes:**
- User namespace: map root to nobody outside, give worker UID 0 inside
- Mount namespace: private, no new mounts
- PID namespace: isolate process IDs

### 3. Windows (`cfg(target_os = "windows")`)

**Dependencies:** `windows-sys` (already in tree via transitives).

**Lockdown sequence (parent-side, before spawn):**
1. Create restricted AppContainer token via `CreateAppContainerProfile`
2. Add allowed capabilities: IPC socket (already inherited), document handle, shmem handle
3. Set job object with:
   - `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` — kill worker when coordinator exits
   - `JOB_OBJECT_LIMIT_PROCESS_MEMORY` — cap per-process memory
   - `JOB_OBJECT_CPU_RATE_HARD_CAP` — cap CPU
4. Assign child process to job object at creation
5. Apply AppContainer token to child

**Transport change required:**
TCP loopback (`127.0.0.1`) is blocked by AppContainer network restrictions.
Must switch to inherited named pipe HANDLE before confinement is applied.
This is a prerequisite, not a future task.

### 4. macOS (`cfg(target_os = "macos")`)

**Dependencies:** `libc` for `sandbox_init` FFI.

**Lockdown sequence (child-side):**
1. Load sandbox profile (compiled SBPL or `sandbox_init` with profile string)
2. Profile denies: network, filesystem (except inherited handles), process spawn,
   sysctl writes, device access
3. Profile allows: mach IPC (for inherited handles), basic memory management

### 5. Integration Points

**worker-main main.rs** — add at the very start, before `adopt_inherited()`:
```rust
fn main() -> ExitCode {
    // Apply sandbox confinement BEFORE any handle adoption or untrusted input.
    // SECURITY: human-gated — do not weaken filters. [ADR-016, IG AI-6]
    if let Err(e) = sandbox::confinement::lockdown_worker() {
        eprintln!("worker: confinement failed: {e}");
        return ExitCode::from(1);
    }
    // ... rest of main (adopt_inherited, etc.)
}
```

**coordinator spawn** — add on Windows only:
```rust
// On Windows, apply AppContainer before spawning the child.
#[cfg(target_os = "windows")]
sandbox::confinement::confine_child(&mut command)?;
```

### 6. What needs human review

- [ ] The seccomp-bpf syscall allowlist (every allowed syscall is an attack surface)
- [ ] The AppContainer capability list (which capabilities to grant)
- [ ] The macOS sandbox profile (what's denied vs. allowed)
- [ ] The namespace configuration (user/mount/pid namespace choices)
- [ ] Memory and CPU limits for the job object
- [ ] The transport change from TCP loopback to named pipe (Windows)
- [ ] Whether the allowlist is truly fail-closed (unknown syscalls → kill)

### 7. Non-goals (deferred)

- Full seccomp-bpf BPF program generation (use a minimal allowlist for M0)
- Complete namespace setup (user namespace only for M0, mount/pid deferred)
- Named pipe transport (TCP loopback kept for M0, confinement is advisory)
- Utility worker pool confinement (M1+)
- Plugin sandbox (M2+)

## Risk

**The biggest risk is shipping a confinement that gives a false sense of security.** If the
seccomp allowlist is too permissive, the sandbox is theater. If it's too restrictive, the
worker can't function. The M0 approach is:

1. Ship with confinement as **advisory + logged** (not fatal if it fails)
2. Document exactly what's allowed and what's denied
3. Mark as "M0 draft — needs security review"
4. Never claim "sandboxed" until the filters are reviewed and tested

This matches ADR-016's honesty value: "permission flags are honored by default but
documented as advisory."
