# Design: Worker session + death detection (M0 slice 3)

**Date:** 2026-07-12  
**Milestone:** M0 — walking skeleton  
**Depends on:**  
- Slice 1 — `WorkerTransport` (merged PR #3)  
- Slice 2 — `spawn_worker` + ping/echo (merged PR #4)  

**Cites:** ADR-008, ADR-010, ADR-021, ADR-022, ADR-025, SDS §2.2 session notes,
§3.1 step 3, §4.2, §7, §10.1 (detection only this slice), §14 M0  
**Transport law:** `docs/superpowers/specs/2026-07-12-worker-transport-design.md`  
**Spawn law:** `docs/superpowers/specs/2026-07-12-worker-spawn-design.md`

---

## Goal (plain language)

We can already **spawn a worker** and talk to it. This slice makes the
**coordinator** (thin session helper) **notice when the worker dies** and
surface that as a typed event — the first half of SDS §10.1.

Full §10.1 also requires respawn + re-broker file + re-parse + re-render.
Those need brokered handles and engine bootstrap. **Not this slice.**

After this slice:

1. Session owns a `WorkerChild`.
2. Death is detected via **IPC disconnect and/or process exit**.
3. A `WorkerDied` (or equivalent) event is produced for upper layers.
4. Tests kill the worker and assert detection (and optional ping-only respawn).

## Why this step (do not skip)

| If we skip… | Failure mode |
|-------------|--------------|
| Design | Invent event shapes / respawn that fight SDS later |
| Protocol event first | Session emits ad-hoc strings; shell can't bind |
| Session without tests | CI never proves kill-detection on 3 OS |
| Full §10.1 now | Forces PDFium + file broker before death signal exists |

## Scope

### In

1. **`protocol::events`** — minimal typed event for worker death (see shape below).
2. **`coordinator::session`** — small API:
   - attach / spawn a worker (reuse `sandbox::spawn::spawn_worker`)
   - `poll` / `pump` that checks transport + child status
   - on death: record state, yield `WorkerDied`
3. **Optional minimal respawn (ping-only):** after death, session may spawn a
   **new** worker and re-establish IPC for tests — **no** file handle, **no**
   overlay replay. Document as "respawn skeleton," not SDS-complete recovery.
4. **Tests:** unit/integration kill → `WorkerDied`; optional respawn → ping works again.

### Out (explicit — next slices)

| Item | Why deferred |
|------|----------------|
| Brokered file handle / mmap | Needs broker slice |
| Bootstrap parse / structural summary | Needs worker PDF path |
| Overlay replay / journal | M3 mutation core |
| Full circuit breaker / diagnostics UI | Product surface later |
| OS confinement | Human-gated (IG) |
| Shell / cxx / Qt binding of events | Above coordinator |
| Shmem tiles / PDFium | Later M0 |

## Detection rules (normative)

SDS §10.1: *“Detected by the coordinator via IPC channel death / process-exit signal.”*

Both signals are valid; implement **either-or** and prefer honesty:

| Observation | Meaning |
|-------------|---------|
| `recv_timeout` → `Disconnected` | Peer closed IPC (worker exited or crashed) |
| `Child::try_wait` → `Some(status)` | Process exited (may race ahead of IPC EOF) |
| `recv_timeout` → `Timeout` alone | **Not** death — worker may be busy (later) or idle |

**Algorithm for one `poll` call (blocking budget = short timeout):**

```text
1. If child.try_wait()?.is_some() → mark Dead, emit WorkerDied { reason: ProcessExited(status) }
2. Else match transport.recv_timeout(poll_timeout):
     - Ok(frame) → deliver to caller / ignore if unsolicited (M0: unexpected frames logged or returned)
     - Err(Timeout) → Alive, no event
     - Err(Disconnected) → mark Dead, emit WorkerDied { reason: IpcDisconnected }
     - Err(other) → mark Dead or Failed with reason Io (honest; do not pretend Alive)
3. Once Dead, further poll returns Dead without re-emitting unless respawned
```

M0 does **not** require a dedicated watcher thread. A **synchronous poll** called
from tests (and later from the coordinator actor loop) is enough (GR-6: threads +
channels later; no async runtime).

## Event shape (protocol)

`protocol/src/events.rs` is currently a stub. Introduce the **minimum** needed:

```text
pub enum CoordinatorEvent {
    /// Worker process for a document/session is gone. [SDS §10.1]
    WorkerDied {
        /// Opaque session/document id for M0 (u64 is enough).
        session_id: u64,
        reason: WorkerDeathReason,
    },
    // Other events remain for later slices
}

pub enum WorkerDeathReason {
    IpcDisconnected,
    ProcessExited { code: Option<i32> },
    Io(String), // display-only; avoid exposing raw io::Error across FFI yet
}
```

**Decisions:**

- No bincode wire codec yet (same as transport: opaque at IPC; events are
  **in-process** coordinator→shell types for now).
- `session_id` is a local counter / handle; not stable across app restarts.
- Do **not** invent `DocumentOpened` etc. here unless a test needs them.

## Session API sketch

```text
// coordinator::session

pub struct WorkerSession {
    id: u64,
    state: SessionState, // Alive { child: WorkerChild } | Dead { reason } | ...
    worker_exe: PathBuf,
}

impl WorkerSession {
    pub fn spawn(worker_exe: &Path) -> io::Result<Self>;
    pub fn session_id(&self) -> u64;
    pub fn is_alive(&self) -> bool;
    /// Short poll; may return events. Does not block longer than `timeout`.
    pub fn poll(&mut self, timeout: Duration) -> Result<Vec<CoordinatorEvent>, SessionError>;
    /// Kill child (test / fault injection). [ADR-022]
    pub fn kill_worker(&mut self) -> io::Result<()>;
    /// Ping-only respawn after death (optional capability; no file broker).
    pub fn respawn_ping_only(&mut self) -> io::Result<()>;
}
```

Names may vary in the plan; behavior above is binding.

## Relation to full SDS §10.1

| §10.1 step | This slice |
|------------|------------|
| 1. Mark worker dead; abandon in-flight gens | Mark dead + event; no generation system yet |
| 2. Respawn + re-transfer handle + bootstrap | **Out** except optional ping-only respawn |
| 3. State reconstruction from file+overlay+journal | **Out** |
| 4. Reissue tiles + UI notice + diagnostics | **Out** |

## Testing (ADR-022)

1. **Spawn + poll timeout:** alive, no events.
2. **Kill + poll:** eventually `WorkerDied` (disconnect and/or exit).
3. **Idempotent:** second poll after death does not spam duplicate events
   (or documents a single-shot flag — prefer single emit).
4. **Optional:** `respawn_ping_only` → send/recv `ping` via session helper or
   exposed transport for test only.

Prefer tests under `coordinator` with `CARGO_BIN_EXE_worker` if the package can
see the bin (dev-dep / workspace test), or keep spawn tests in `worker-main` and
**unit-test** session logic with a mock `WorkerTransport` + fake child if process
tests are awkward. **Prefer real process** (matches M0 risk) when feasible.

## Success criteria

- [ ] Design + plan committed or present before implementation PR.
- [ ] `WorkerDied` (+ reason) exists in `protocol`.
- [ ] `WorkerSession` (or equivalent) detects kill on CI (3 OS).
- [ ] No claim of full transparent recovery / sandboxed worker.
- [ ] No new crates.io deps without ADR-028 note.
- [ ] No confinement, PDFium, shmem, Qt.

## Risks

| Risk | Mitigation |
|------|------------|
| Race: exit vs IPC EOF order | Accept either signal; tests allow either reason |
| Blocking forever on recv | Always use `recv_timeout` with caller budget |
| Scope creep into OpenDocument | Hard out-list; reject in review |
| Duplicate WorkerDied | Single-shot until respawn |

## Next slices (ordered)

1. **Broker file handle + worker mmap + structural summary** (open path start).
2. **Respawn with re-open** (complete §10.1 steps 2–3 without tiles).
3. **Confinement draft** (human-gated).
4. **Shmem + tile + engine**.
5. **Shell event binding**.

---

*Design only until the implementation plan tasks are executed. Does not amend ADR constitution.*
