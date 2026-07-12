# Design: Kill-worker transparent respawn with document re-open (M0 slice 6)

**Date:** 2026-07-12  
**Milestone:** M0  
**Depends on:** slices 1–5 on `main` (through PR #7)  
**Cites:** ADR-008, ADR-021, ADR-022, SDS §10.1 steps 1–3 (partial), §14 M0 exit
(“kill-the-worker test shows transparent respawn”)

---

## Goal

Prove the M0 **kill → detect → respawn → document usable again** path:

1. Session owns a brokered document + live worker.
2. Worker is killed (fault injection).
3. Session emits `WorkerDied` (existing).
4. Session **respawns** a fresh worker and **re-inherits** the same open file handle.
5. `inspect` works again with the same structural summary shape.

No PDFium, tiles, overlay replay, or confinement.

## Why this step

SDS §14 M0 exit criteria require kill-the-worker transparent respawn. We already
have death detection and ping-only respawn; document re-open was missing.

## Scope

### In

- `WorkerSession` **owns** `Option<BrokeredFile>` (API: take `BrokeredFile` by value).
- Unified `respawn()`:
  - if document present → `spawn_worker_with_file` again
  - else → plain `spawn_worker` (replaces/covers `respawn_ping_only`)
- Keep `respawn_ping_only` as thin alias **or** remove and update tests to `respawn`.
- Integration test: inspect → kill → poll death → respawn → inspect again.

### Out

| Item | Why |
|------|-----|
| Overlay / journal replay | No mutation core yet |
| Tile reissue | No render path yet |
| Circuit breaker / UI notice | Later product surface |
| Automatic respawn without call | Coordinator actor loop later; tests call `respawn()` |

## API change

```text
// before
spawn_with_document(exe, &BrokeredFile) -> ...

// after
spawn_with_document(exe, BrokeredFile) -> ...  // session owns the file
respawn(&mut self) -> Result<(), SessionError> // dead only; reattach doc if any
```

`session_id` remains stable across respawn (same session, new worker process).

## Success criteria

- [ ] Design + plan present
- [ ] Kill + respawn + re-inspect green on 3-OS CI
- [ ] No path string reintroduced to Z1
- [ ] No PDFium / shmem / confinement

## Next

Shared-memory tile seam, or confinement draft (human-gated), or CLI multi-process path.

---

*Design only until plan executes.*
