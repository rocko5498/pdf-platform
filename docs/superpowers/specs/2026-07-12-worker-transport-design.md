# Design: WorkerTransport + length-prefix framing (M0 slice 1)

**Date:** 2026-07-12  
**Milestone:** M0 — walking skeleton  
**Cites:** ADR-031 (IPC transport), ADR-008 (multi-process), ADR-010 / GR-6 (no async
runtime), ADR-016 (sandbox), ADR-022 (testable seam), ADR-025 (crate layout),
ADR-027 (`unsafe` discipline), ADR-028 (deps), SDS §1.1, §3.1 steps 2–3, §4.2–4.3,
§7.2, §10.1, §14 M0 exit criteria

---

## Goal (plain language)

The desktop app will eventually run PDF parsing/rendering in a **separate sandboxed
process** (the “worker”), while a trusted “coordinator” stays in the UI process.
They need a **phone line** between them: small control messages go over that line;
big pixel buffers will later use shared memory (not this slice).

This design defines that phone line: **`WorkerTransport`** — how bytes are framed and
moved on Linux/macOS/Windows, with tests that do not need the full PDF engine yet.

## Why this slice before spawn / PDFium / sandbox lockdown

SDS §14 M0 needs: tile via bridge + IPC + shmem, sandboxed worker, kill-respawn.
Those stack. The bottom of the stack is **a reliable, cross-platform control channel**
with crash detection via disconnect (ADR-031 §6, SDS §10.1). Without it, spawn and
sandbox code has nothing to hang on.

We deliberately **do not** implement full OS confinement, PDFium, or shmem here.
Those are later M0 slices that depend on this seam.

## Scope

### In (this design / following plan)

1. **`protocol::transport::WorkerTransport` trait** finalized (timeout receive is
   required for worker IPC thread behavior — SDS §7 / ADR-031 §6).
2. **4-byte little-endian length-prefix framing** helpers (encode/decode frames).
3. **`LoopbackTransport`** (in-process pair) for unit tests and coordinator logic
   without real processes (ADR-031 consequences: testable seam; SDS §7.5 style).
4. **Platform impls in `sandbox`:**
   - Unix (`AF_UNIX` + `SOCK_STREAM`) — Linux and macOS
   - Windows named pipes (`\\.\pipe\<uuid>`)
5. **Tests:** framing unit tests; loopback round-trip; OS-level pair smoke test
   (parent↔child or two ends on one machine) where CI can run them.

### Out (explicitly deferred)

| Item | Why deferred |
|------|----------------|
| Typed messages / bincode serialization | ADR-031: separate decision; transport carries opaque `&[u8]` |
| Shared-memory tile buffers | Later slice; descriptors ride *inside* frames later |
| Worker spawn + handle inheritance | `sandbox::spawn` next slice; needs this transport first |
| Sandbox confinement (seccomp / AppContainer) | Security-critical, human-gated (IG); after spawn works |
| `worker-main` PDF bootstrap / PDFium | Needs engine prebuilts + spawn |
| Shell / cxx bridge | Above coordinator |
| Heartbeats | ADR-031: disconnect is crash signal; no heartbeat required |

## Decisions (bound by ADR-031; no re-litigation)

| Topic | Decision |
|-------|----------|
| Mechanism | Unix domain sockets (Linux/macOS); named pipes (Windows) |
| Framing | `u32` LE length + body; max frame size **bounded** (see below) |
| Trait location | `protocol` crate only — no OS types |
| Impl location | `sandbox` crate, `#[cfg(target_os = ...)]` |
| Sync model | Threads + blocking I/O; **no** async runtime (GR-6) |
| Crash signal | `recv_*` returns `Disconnected` on EOF / broken pipe |
| Timeout | `recv_timeout` on the trait (worker IPC thread must not block forever) |

### Trait shape (normative for implementation)

ADR-031 names `send_frame` / `recv_frame`. The existing stub correctly adds timeout
for SDS worker-thread needs. Normative API for M0:

```text
trait WorkerTransport: Send + 'static {
    fn send(&mut self, frame: &[u8]) -> Result<(), TransportError>;
    fn recv_timeout(&mut self, timeout: Duration) -> Result<Vec<u8>, TransportError>;
}
```

- `send` writes one complete framed message (may block until accepted by the OS buffer).
- `recv_timeout` returns one complete frame, `Timeout`, `Disconnected`, or `Io`.
- Optional convenience: default method or free function `recv_blocking` via a long
  timeout is **not** required in M0.

### Max frame size (SDS-aligned bound)

GR-7 / memory discipline: transports must refuse absurd lengths (malicious or corrupt
peer). **M0 bound: 16 MiB** per control frame. Control messages are small; anything
larger is a protocol bug or attack. Shmem will carry pixels later, not this channel.

On decode: if length prefix `> MAX_FRAME` → `TransportError::Io` or a dedicated
`Protocol` variant; implementation may add `TransportError::FrameTooLarge` — prefer
one explicit variant for honest diagnostics (ADR-020 spirit).

### Establishment sequence (this slice vs next)

**This slice** only implements “two ends of a connected transport exist and can
exchange frames.” How they are connected:

| Mode | Purpose |
|------|---------|
| Loopback | Unit tests; no OS sockets |
| `pair()` helper (platform) | Create connected pair in-process (socketpair / connected pipe) for tests |
| Real spawn inherit | **Next slice** (`sandbox::spawn`): listen → spawn worker with inherited handle → accept |

M0 product path will use inherit-before-sandbox (ADR-031 §2). Tests may use `pair()`
without spawning `worker-main`.

### Error mapping

| Condition | `TransportError` |
|-----------|------------------|
| Peer closed / broken pipe | `Disconnected` |
| Timeout elapsed, no full frame | `Timeout` |
| Partial read then timeout | Implementation must not corrupt framing state (buffer leftover or reset — prefer buffer until complete frame or disconnect) |
| Length > MAX | `FrameTooLarge` (add variant) or map to `Io` with clear message — **prefer new variant** |
| Other OS errors | `Io` |

### Partial reads / writes

Framing layer must reassemble across multiple `read` syscalls. Writers must loop
until the full length-prefix + body is written (or fail). No message boundary
reliance on `SOCK_SEQPACKET` (unavailable on macOS — ADR-031 trade-offs).

### Dependencies

- Prefer **std only** for Unix sockets and Windows named pipes if practical.
- If Windows named-pipe ergonomics need a small crate, flag it under ADR-028
  (license + exit seam) before adding — default plan: `std::os::windows` + `windows-sys`
  only if already forced by other code; otherwise raw `std` / minimal `windows-sys`.
- **No** tokio, async-std, ipc-channel (ADR-031 alternatives rejected).

### Security notes (honest limits of this slice)

- Transport alone does **not** sandbox the worker.
- Named pipe ACLs / socket permissions for production spawn are part of the **spawn
  + confinement** slice; `pair()` tests run unsandboxed in CI.
- Z1 output remains untrusted at higher layers (SDS invariant); transport only moves
  bytes.

## Architecture

```
protocol/
  transport.rs     # TransportError, WorkerTransport, framing, LoopbackTransport
sandbox/
  transport.rs     # UnixWorkerTransport / WindowsWorkerTransport + pair()
  spawn.rs         # OUT OF SCOPE this slice (stub remains)
  confinement.rs   # OUT OF SCOPE this slice (stub remains)
```

```
Coordinator (later)                    Worker (later)
      |                                      |
      |  WorkerTransport::send(frame)        |
      |------------------------------------->|
      |  WorkerTransport::recv_timeout(...)  |
      |<-------------------------------------|
      |                                      |
  Disconnected == worker dead (SDS §10.1)
```

## Testing plan (ADR-022 strata applicable)

1. **Unit:** encode/decode framing; reject oversize; split-buffer reassembly.
2. **Unit:** loopback send/recv_timeout / disconnect behavior.
3. **Integration (cfg):** platform `pair()` echo between two threads.
4. **CI:** existing 3-OS matrix already runs `cargo test --workspace`; new tests
   must pass there without qpdf (transport is independent).

## Success criteria for “slice 1 done”

- [ ] Trait + framing + loopback land in `protocol`, documented.
- [ ] Unix + Windows impls in `sandbox` with `pair()` or equivalent test path.
- [ ] `cargo test -p protocol -p sandbox` green on CI (3 OSes).
- [ ] No new crates.io dependency without ADR-028 note in the PR.
- [ ] No claim of “sandboxed worker” yet — only “IPC control channel works.”

## Next slices (ordered)

1. **Spawn + inherit** (`sandbox::spawn`, minimal `worker-main` echo/ping).
2. **Kill-worker detection** via `Disconnected` + coordinator session stub.
3. **Confinement draft** (human-gated).
4. **Shmem pool + one tile** + engine.
5. **ffi-bridge + shell composite** (thinnest path to “page 1 visible”).

---

*Design only until implementation plan tasks are executed. Does not change ADR-031.*
