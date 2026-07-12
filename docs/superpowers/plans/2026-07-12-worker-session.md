# Implementation Plan: Worker session + death detection (M0 slice 3)

> **For agentic workers:** Execute task-by-task. Do not skip tasks. Checkboxes track progress.  
> Design: `docs/superpowers/specs/2026-07-12-worker-session-design.md`

**Goal:** Typed `WorkerDied` + coordinator session that detects worker kill.  
**Not goal:** Full SDS §10.1 recovery, confinement, PDFium, file broker.

**Cites:** ADR-008, ADR-010, ADR-021, ADR-022, ADR-025, SDS §10.1 (detect), §14 M0  

**Branch:** `feat/m0-worker-session` from current `main` (after PR #4).

---

### Task 0: Preconditions (do not skip)

- [ ] Confirm `main` contains transport + spawn (PR #3, #4).
- [ ] Read design doc fully.
- [ ] Branch from `main`.
- [ ] Log intent in `.agent-state/log.md` + claim in `claims.md`.

---

### Task 1: Protocol events — `WorkerDied`

**Files:**
- Modify: `core/protocol/src/events.rs`
- Modify: `core/protocol/src/lib.rs` if re-exports needed

**Steps:**

1. Define `WorkerDeathReason` and `CoordinatorEvent::WorkerDied { session_id, reason }`
   (or free-standing structs if enum-of-all-events is premature — prefer one
   `CoordinatorEvent` enum so shell has a single type later).
2. `Debug` + docs with SDS §10.1 cite.
3. Unit test: construct and match event (smoke).
4. **Commit:** `feat(protocol): WorkerDied coordinator event (SDS §10.1 detect)`

---

### Task 2: `WorkerSession` API in coordinator

**Files:**
- Modify: `core/coordinator/src/session.rs`
- Modify: `core/coordinator/src/lib.rs` only if docs/exports need touch

**Steps:**

1. Replace ponytail stub with `WorkerSession` per design.
2. `spawn(worker_exe)`, `poll(timeout)`, `kill_worker()`, `is_alive()`, `session_id()`.
3. Single-shot `WorkerDied` emission.
4. Map `ChildExit` / `Disconnected` / `Io` to `WorkerDeathReason`.
5. **No** confinement, **no** file broker.
6. **Commit:** `feat(coordinator): WorkerSession poll detects worker death`

---

### Task 3: Optional ping-only respawn

**Files:** `core/coordinator/src/session.rs`

**Steps:**

1. `respawn_ping_only()` only valid from `Dead` state.
2. Spawns new worker; resets to Alive; clears single-shot flag.
3. If this expands the PR too much, **skip** and leave a `// ponytail:` note —
   detection alone satisfies slice success criteria.
4. **Commit (if done):** `feat(coordinator): ping-only worker respawn skeleton`

---

### Task 4: Tests (required)

**Files:**
- Prefer: `core/coordinator/tests/worker_session.rs`  
  OR unit tests in `session.rs` if bin discovery is hard.

**Steps:**

1. Ensure `worker` binary is available (`CARGO_BIN_EXE_worker` via dev-dep on
   workspace bin — if Cargo cannot express this, use
   `env::var("CARGO_BIN_EXE_worker")` from a test in `worker-main` that drives
   `WorkerSession`, **or** path under `target/debug/worker` with
   `cargo test -p worker-main` building the bin first).

   **Chosen approach (fill in during impl):** simplest path that CI runs.

2. Test: spawn → poll timeout → alive.
3. Test: spawn → kill → poll → `WorkerDied` once; second poll no duplicate.
4. Test (if Task 3 done): respawn → ping via transport if exposed for test.

5. **Commit:** `test(coordinator): WorkerSession death detection`

---

### Task 5: Workspace verify

1. `cargo test -p protocol -p sandbox -p worker-main -p coordinator --all-targets`
2. `cargo build --workspace`
3. Fix 3-OS issues (macOS disconnect mapping already in transport).

---

### Task 6: PR (do not skip; do not auto-merge unless user asks)

1. Push branch.
2. Open PR with citations + honest limits (“detection only, not full §10.1”).
3. Wait for CI green.
4. **Stop for human merge** unless user explicitly says to merge.

---

## Non-goals checklist

- [ ] No file broker / mmap
- [ ] No PDFium / tiles / shmem
- [ ] No full overlay replay
- [ ] No confinement
- [ ] No Qt / cxx event binding
- [ ] No “M0 complete” claim

## After merge

Next design: open-document path start (broker handle + worker adopt file) **or**
complete §10.1 respawn with re-open — design-before-code again.
