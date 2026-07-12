# Implementation Plan: Document handle inherit (M0 slice 5)

> Design: `docs/superpowers/specs/2026-07-12-handle-inherit-design.md`

**Branch:** `feat/m0-handle-inherit` from `main`

---

### Task 0
- [ ] Sync main, branch, log/claim, read design

### Task 1: sandbox inherit helpers + spawn_worker_with_file
**Files:** `core/sandbox/src/spawn.rs` (maybe `inherit.rs`)

1. Unix: `clear_cloexec(fd)` via fcntl extern + SAFETY
2. Windows: `set_inheritable(handle)` via SetHandleInformation + SAFETY
3. `spawn_worker_with_file(exe, &File, extra_env)`
4. `adopt_document_file() -> io::Result<Option<File>>`
5. Deprecate/remove `ENV_DOC_PATH` from spawn path
6. Commit: `feat(sandbox): inherit document FD/HANDLE into worker`

### Task 2: session + worker
**Files:** `session.rs`, `worker-main/src/main.rs`

1. `spawn_with_document` → `spawn_worker_with_file`
2. Worker: `adopt_document_file` + `scan_file`
3. Commit: `feat(session,worker): inspect via inherited document handle`

### Task 3: tests
1. `open_inspect` still green
2. Optional: spawn without file, inspect fails
3. Commit if needed: `test: handle-inherit open inspect`

### Task 4: verify + PR + CI + merge
- cargo test relevant packages
- PR with GR-1 debt closed note
- Merge after green (user said go ahead)

### Non-goals
- No IPC socket inherit change
- No confinement
- No new deps
