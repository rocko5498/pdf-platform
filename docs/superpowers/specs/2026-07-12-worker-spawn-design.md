# Design: Worker spawn + handle inherit + ping (M0 slice 2)

**Date:** 2026-07-12  
**Milestone:** M0 — walking skeleton  
**Depends on:** slice 1 (`WorkerTransport` framing + pair tests) — PR #3  
**Cites:** ADR-008, ADR-010 (GR-6), ADR-016, ADR-022, ADR-025, ADR-027, ADR-028,
SDS §2.1 cold start / open path, §3.1 steps 2–3, §4.2, §7, §10.1, §14 M0

> Note: design docs and the WorkerTransport plan refer to **ADR-031** for the IPC
> control-channel decision. That number is **not yet present** in
> `docs/adr-constitution.md` (constitution ends at ADR-030). Until a superseding
> ADR lands, treat the accepted decisions in
> `docs/superpowers/specs/2026-07-12-worker-transport-design.md` as the binding
> transport law for M0. Do **not** invent ADR text in code.

---

## Goal (plain language)

Slice 1 gave us a **phone line** (framed duplex bytes). Slice 2 makes a **real
child process** hold the other end of that line:

1. Coordinator (or a thin test harness) **creates** the channel.
2. **Spawns** `worker` with the channel end **inherited**.
3. Worker **adopts** the inherited end and runs a tiny **ping/echo** loop.
4. Parent sees **disconnect** when the worker exits (crash signal for later
   kill-respawn work — SDS §10.1).

Still **no** PDF parsing, PDFium, shared memory, or OS confinement lockdown.

## Why this order

SDS §3.1: broker → **spawn** → mmap → parse → …  
SDS §14 M0 needs sandboxed worker + kill-respawn. Those need:

| Prerequisite | Slice |
|--------------|-------|
| Framed IPC | 1 (done / in PR) |
| Process + inherit + ping | **2 (this design)** |
| Disconnected → session event | 3 |
| Confinement (human-gated) | 4 |
| Shmem + tile + engine | 5+ |

Spawn without confinement is honest: we prove process topology and inheritance
before locking the worker down (security-critical draft later, IG human-gate).

## Scope

### In

1. **`sandbox::spawn`** (or similarly named) API that:
   - Creates a connected transport pair (platform-native for production path).
   - Spawns the `worker` binary with **one end inherited**, parent keeps the other.
   - Returns a parent-side `WorkerTransport` + child process handle/id.
2. **`worker-main`**: replace `todo!` with:
   - Adopt inherited channel.
   - Loop: `recv_timeout` → if body is a known ping payload, echo it back;
     exit cleanly on disconnect or a quit message.
3. **Integration test** (CI-safe): parent spawns worker, ping round-trip, drop
   parent / kill child, observe disconnect.
4. **Platform inherit mechanics** (minimal, std-first):
   - **Unix:** socketpair / already-connected `UnixStream`; pass FD via
     `std::os::fd` + `Command` with `pre_exec` or clear-cloexec + env
     `PDF_PLATFORM_IPC_FD=<n>` (exact mechanism chosen in plan; prefer least
     `unsafe`).
   - **Windows:** **named pipe** (product path per transport design). Parent
     creates server pipe, spawns child with pipe name or inherited handle;
     retire TCP loopback for *spawned* workers. TCP loopback may remain for
     in-process unit tests only.

### Out (explicit)

| Item | Why |
|------|-----|
| seccomp / AppContainer / Seatbelt | Security-critical; human-gated draft after spawn works |
| Brokered file handle / mmap | Needs open path; next after ping |
| PDFium / rasterize | Engine prebuilts + trait wire-up later |
| Shmem tiles | After control channel + process proven |
| Typed bincode protocol | Still opaque `&[u8]` frames |
| Coordinator full session | Thin harness / test is enough for slice 2 |
| Qt / cxx | Above coordinator |

## Decisions for this slice

| Topic | Decision |
|-------|----------|
| Who owns spawn | `sandbox` crate (already named for confinement + transport impls) |
| Binary name | `worker` (`worker-main` package `[[bin]]`) |
| Discovery of worker path | Env override `PDF_PLATFORM_WORKER_PATH` for tests; else next to parent exe or `CARGO_BIN_EXE_worker` in integration tests |
| Message content | Opaque bytes; test uses fixed `b"ping"` / echo same bytes — **no** schema yet |
| Timeout | Parent and worker use `recv_timeout` (seconds-scale in tests); worker exits if idle beyond a long optional deadline **or** simply blocks on long timeout until disconnect (prefer: loop with 1s timeout, ignore pure Timeout, exit on Disconnected) |
| Crash signal | Child death → parent `recv_*` → `Disconnected` (already mapped in transport) |
| Async | **Forbidden** in core (GR-6); `std::process::Command` + threads only |
| New deps | Prefer std. Windows named-pipe create may need `windows-sys` — **flag ADR-028** in PR if added; license + exit seam: “replace with std when available / isolate in `sandbox::windows_pipe` module” |

### Inherit-before-sandbox (product sequence, partial here)

SDS / ADR-008 intent: establish IPC **before** confinement so the locked-down
worker never needs broad “open socket” rights.

This slice implements **inherit before any lockdown call**. Confinement module
stays stub; the **call order** is documented so slice 4 plugs in:

```text
1. create channel pair
2. spawn worker with inherited end   ← this slice
3. (later) worker or parent applies confinement
4. (later) broker file handle into worker
```

Do **not** call confinement APIs in slice 2.

### API sketch (normative direction; names flexible in plan)

```text
// sandbox
struct WorkerChild {
    transport: /* platform WorkerTransport */,
    child: std::process::Child,
}

fn spawn_worker(exe: &Path) -> io::Result<WorkerChild>;

// worker-main
fn main() {
    let mut t = adopt_inherited_transport().expect("ipc");
    loop {
        match t.recv_timeout(Duration::from_secs(1)) {
            Ok(msg) if msg == b"quit" => break,
            Ok(msg) => { let _ = t.send(&msg); } // echo
            Err(Timeout) => continue,
            Err(Disconnected) => break,
            Err(e) => { /* log to stderr for M0; exit 1 */ break; }
        }
    }
}
```

Platform-specific `adopt_inherited_transport` lives in `sandbox` and is used by
`worker-main` so adopt logic is not duplicated.

## Error / honesty rules

- Spawn failure → typed `io::Error` / sandbox error; never silent.
- Missing inherit env/handle → worker exits non-zero with stderr message.
- Partial frames still handled by existing `FrameDecoder` (slice 1).
- Do not claim “sandboxed” in logs or PR title for this slice.

## Testing plan (ADR-022)

1. **Unit (if any):** path resolution helpers; env parse.
2. **Integration:** `spawn_worker` + ping/echo (all 3 OS CI).
3. **Integration:** parent drops transport / kills child → other side sees
   `Disconnected` or process exit.
4. **No** corpus-diff / qpdf dependency for these tests.

## Success criteria

- [ ] `worker` binary builds in workspace.
- [ ] Parent can spawn worker and complete ≥1 echo round-trip on Linux, macOS, Windows CI.
- [ ] Worker exit / kill surfaces as transport disconnect or observed child status.
- [ ] No confinement, no PDFium, no new network/telemetry.
- [ ] Any new Windows dep documented under ADR-028 in the PR body.

## Risks

| Risk | Mitigation |
|------|------------|
| FD/HANDLE inherit footguns (CLOEXEC, handle flags) | Centralize in `sandbox`; integration test is the contract |
| Windows named pipe race (connect before listen) | Document connect order; unique pipe name per spawn |
| Test finds wrong `worker` binary | Prefer `CARGO_BIN_EXE_worker` in `#[test]` |
| Scope creep into “real open document” | Hard non-goals list above |

## Next slices after this

1. Kill-worker detection + coordinator session stub (`WorkerDied` event).
2. Confinement draft (human-gated).
3. Brokered file handle + mmap read-only.
4. Structural summary path (may already partially exist on CLI branch).
5. Shmem + one tile + engine.

---

*Design only. Does not implement spawn. Does not amend the ADR constitution.*
