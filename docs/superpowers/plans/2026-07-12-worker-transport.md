# Implementation Plan: WorkerTransport (M0 slice 1)

> **For agentic workers:** Implement task-by-task. Checkboxes track progress.
> Design: `docs/superpowers/specs/2026-07-12-worker-transport-design.md`

**Goal:** Land a tested, cross-platform control-channel seam (`WorkerTransport` +
length-prefix framing + loopback + OS impls) without spawn, sandbox lockdown, or PDFium.

**Cites:** ADR-031, ADR-008, ADR-010, ADR-022, ADR-025, ADR-027, ADR-028, SDS §4.2, §10.1, §14

**Branch:** `feat/m0-worker-transport` (from `main` @ PR #2 merge)

---

### Task 1: Framing + trait finalize in `protocol`

**Files:**
- Modify: `core/protocol/src/transport.rs`
- Modify: `core/protocol/src/lib.rs` (exports if needed)

**Steps:**

1. Add `TransportError::FrameTooLarge { max: u32, got: u32 }` (or equivalent).
2. Keep `WorkerTransport` with `send` + `recv_timeout` (align comments with ADR-031).
3. Implement framing helpers (same module or `protocol::transport::framing`):
   - `const MAX_FRAME: u32 = 16 * 1024 * 1024;`
   - `fn encode_frame(body: &[u8]) -> Result<Vec<u8>, TransportError>`
   - `fn decode_length(prefix: [u8; 4]) -> Result<usize, TransportError>`
   - Buffered reader helper type optional: `FrameReader` holding incomplete bytes.
4. Unit tests: empty body; small body; max-1; reject max+1; multi-chunk reassembly if
   helper exists.
5. Commit: `feat(protocol): WorkerTransport framing and error variants`

---

### Task 2: Loopback transport

**Files:**
- Modify: `core/protocol/src/transport.rs` (or `loopback.rs` + mod)

**Steps:**

1. `LoopbackTransport` backed by `std::sync::mpsc` **or** a pair of
   `std::os::unix`/`windows` only if needed — prefer pure Rust channels:
   - Actually mpsc gives messages but not timeout on both ends easily.
   - Prefer: `pair_loopback() -> (LoopbackEnd, LoopbackEnd)` using
     `crossbeam-channel`? **No new dep** — use `std::sync::mpsc` +
     `recv_timeout` on the receiver (std supports `recv_timeout`).
2. Two ends: each has send queue to the other.
3. Tests: round-trip; timeout when idle; disconnect when other end dropped.
4. Commit: `feat(protocol): LoopbackTransport for in-process tests`

---

### Task 3: Unix domain socket transport (`sandbox`)

**Files:**
- Modify: `core/sandbox/src/transport.rs`
- Modify: `core/sandbox/Cargo.toml` (deps: `protocol` path)

**Steps:**

1. Add `protocol` dependency to `sandbox`.
2. `UnixWorkerTransport` wrapping `std::os::unix::net::UnixStream`.
3. Implement framing read/write with internal buffer for partial reads.
4. `#[cfg(unix)] pub fn unix_pair() -> io::Result<(UnixWorkerTransport, UnixWorkerTransport)>`
   using `UnixStream::pair()`.
5. Tests `#[cfg(unix)]`: echo between threads.
6. Commit: `feat(sandbox): Unix WorkerTransport over AF_UNIX stream`

---

### Task 4: Windows named pipe transport (`sandbox`)

**Files:**
- Modify: `core/sandbox/src/transport.rs` (cfg windows modules)

**Steps:**

1. Implement connected pair for tests:
   - Prefer `std::os::windows` anonymous pipe or named pipe with unique name.
   - `std::process` not required for pair-in-process if we use
     `UnixStream`-equivalent: on Windows, `std::net` doesn't have socketpair;
     use named pipe server+client in one process, or `AnonymousPipe` via
     `std::os::windows::io` if available on MSRV 1.80.
2. Research note for implementer: Rust 1.80+ has limited pipe support; acceptable
   approach is `CreateNamedPipeW` / `ConnectNamedPipe` via `windows-sys` **only if
   std cannot express it** — flag dependency in PR body (ADR-028). Prefer std-first.
3. Tests `#[cfg(windows)]`: echo between threads.
4. Commit: `feat(sandbox): Windows WorkerTransport over named pipes`

---

### Task 5: Workspace verify + CI

**Steps:**

1. `cargo test -p protocol -p sandbox --workspace` from `core/`.
2. Ensure stubs in other crates still build (`cargo build --workspace`).
3. Push PR; confirm 3-OS CI green.
4. Commit only if small fixes needed: `test(sandbox): ...` / `fix: ...`

---

### Task 6: Docs touch (minimal)

**Files:**
- Optional one-line in `README.md` Status table: “IPC transport: in progress / landed”

Do **not** claim full M0 walking skeleton complete.

---

## Non-goals checklist (reject scope creep)

- [ ] No `sandbox::spawn` process launch
- [ ] No confinement / seccomp / AppContainer
- [ ] No PDFium / engine-pdfium
- [ ] No bincode message enums
- [ ] No shmem
- [ ] No cxx / Qt

## After this PR merges

Next design: **spawn + inherit + worker-main ping** (slice 2), still without PDFium.
