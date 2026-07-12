# Implementation Plan: Worker spawn + inherit + ping (M0 slice 2)

> **For agentic workers:** Implement task-by-task. Checkboxes track progress.  
> Design: `docs/superpowers/specs/2026-07-12-worker-spawn-design.md`  
> Prerequisite: WorkerTransport slice 1 merged or available on branch.

**Goal:** Spawn real `worker` process with inherited control channel; ping/echo;
disconnect observable. No confinement, no PDFium, no shmem.

**Cites:** ADR-008, ADR-016, ADR-022, ADR-025, ADR-027, ADR-028, SDS §3.1, §4.2, §10.1, §14  
**Transport law:** `docs/superpowers/specs/2026-07-12-worker-transport-design.md`

**Branch:** `feat/m0-worker-spawn` (from `main` after PR #3 merges), unless
continuing on transport branch by explicit human choice.

---

### Task 1: Parent/child adopt API surface in `sandbox`

**Files:**
- Modify: `core/sandbox/src/spawn.rs` (replace stub)
- Modify: `core/sandbox/src/lib.rs` (exports)
- Possibly: `core/sandbox/src/transport.rs` (from_raw / into_stdio helpers)

**Steps:**

1. Define `WorkerChild { transport, child: std::process::Child }` (names flexible).
2. Define `spawn_worker(exe: &Path) -> io::Result<WorkerChild>`.
3. Define `adopt_inherited() -> io::Result<impl WorkerTransport>` for worker side
   (must be `pub` for `worker-main`).
4. Document env vars / handle protocol in module docs:
   - Unix: `PDF_PLATFORM_IPC_FD`
   - Windows: `PDF_PLATFORM_IPC_PIPE` (name) **or** inherited handle (pick one in impl; document)
5. No confinement calls.

**Commit:** `feat(sandbox): spawn_worker and adopt_inherited API surface`

---

### Task 2: Unix implement spawn + inherit

**Files:** `core/sandbox/src/spawn.rs` (`#[cfg(unix)]`)

**Steps:**

1. `UnixStream::pair()` (or listen/accept if inherit requires path — prefer pair).
2. Mark child-end inheritable (clear CLOEXEC) with documented `unsafe` + SAFETY.
3. `Command::new(exe).env("PDF_PLATFORM_IPC_FD", fd).spawn()`.
4. Parent wraps parent-end as `UnixWorkerTransport`.
5. Child `adopt_inherited` reads env, `from_raw_fd`, wraps transport.

**Commit:** `feat(sandbox): Unix worker spawn with inherited AF_UNIX fd`

---

### Task 3: Windows implement spawn + named pipe

**Files:** `core/sandbox/src/spawn.rs` / `transport.rs` (`#[cfg(windows)]`)

**Steps:**

1. Create unique pipe name `\\.\pipe\pdf-platform-<pid>-<n>`.
2. Parent: create server pipe, spawn worker with pipe name in env, connect.
3. Child: connect as client, wrap as `WindowsWorkerTransport`.
4. If `windows-sys` required, add pin + ADR-028 note in PR body; keep usage in one module.
5. Keep TCP `pair()` only for in-process unit tests (slice 1), not for spawn.

**Commit:** `feat(sandbox): Windows worker spawn over named pipe`

---

### Task 4: `worker-main` echo loop

**Files:**
- Modify: `core/worker-main/src/main.rs`
- Modify: `core/worker-main/Cargo.toml` if deps need adjust (no engine required for ping —
  consider default features off for a `ping` smoke binary **or** keep features but don't
  touch engines in `main` yet)

**Steps:**

1. Call `sandbox::spawn::adopt_inherited()` (exact path as implemented).
2. Echo loop per design; honor `b"quit"`.
3. Exit 0 on clean disconnect/quit; non-zero on adopt failure.
4. stderr one-liners only (no tracing dep required unless already present).

**Commit:** `feat(worker-main): adopt IPC and echo ping frames`

---

### Task 5: Integration tests

**Files:**
- Add: `core/sandbox/tests/spawn_ping.rs` **or** `core/sandbox/src/spawn.rs` `#[cfg(test)]`
  using `env!("CARGO_BIN_EXE_worker")` — may require `worker-main` as dev-dep / workspace
  bin discovery. Prefer:

```toml
# sandbox/Cargo.toml
[dev-dependencies]
# ensure worker is built: use required-features or workspace integration test crate
```

  Practical approach used by many workspaces: integration test in `worker-main` or a
  small `tests/spawn_ping.rs` at `core/` level. **Pick simplest that CI runs.**

**Steps:**

1. Spawn → send `b"ping"` → expect `b"ping"` within 5s.
2. Send `b"quit"` or drop → child exits.
3. Kill child mid-flight → parent `recv_timeout` → `Disconnected` (or equivalent).

**Commit:** `test(sandbox): spawn worker ping/echo on platform IPC`

---

### Task 6: Workspace verify + PR

1. `cargo test -p protocol -p sandbox -p worker-main` (and integration).
2. `cargo build --workspace`.
3. PR title must **not** say “sandboxed worker” — say “spawn + inherit + ping”.
4. Confirm 3-OS CI.

---

## Non-goals checklist

- [ ] No seccomp / AppContainer / Seatbelt
- [ ] No file broker / mmap
- [ ] No PDFium / tile / shmem
- [ ] No bincode schemas
- [ ] No Qt / cxx
- [ ] No claim of M0 complete

## After merge

Next design: kill-worker detection + coordinator `WorkerDied` stub (slice 3).
