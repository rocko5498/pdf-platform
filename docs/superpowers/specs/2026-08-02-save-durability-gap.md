# Design: saves are not crash-safe — neither SDS §3.4 path is implemented

**Date:** 2026-08-02
**Milestone:** M3 (the gate for every editing feature, W-2)
**Status:** Report + design. **No save-path code is changed here**; this change only tightens
one fault-injection assertion. IG §2.3 wants the design reviewed before the implementation.
**Cites:** ADR-012, ADR-021, SDS §3.4, SDS §10.3, SDS §10.5, SDS §10.6, SDS §14 M3,
MET-REL-2, MET-REL-3, MET-GOV-2, T-5, T-9, W-2, GR-5, GR-8, AI-6

---

## What the specification requires

SDS §3.4 step 4, on atomicity:

> **Atomicity.** Rename-path (temp + fsync + atomic rename) where supported; append-path
> (journal-intent record → append → commit) for locked/network files, enabling torn-write
> rollback via the intact `/Prev` chain (§10.4).

SDS §10.5:

> rename-path saves are atomic (no torn state possible). Append-path saves write a
> journal-intent record before appending; if the process dies mid-append, next open detects
> the incomplete increment and truncates back to the last valid xref … the file always opens
> as *some* valid revision, never a corrupt hybrid.

SDS §10 states the guarantee outright: **no single failure — worker crash, app crash, or
torn save — loses more than the durability budget of committed work** (≤ 2 seconds or N
commands). Data-loss is one of the absolute metrics under MET-GOV-2, and T-9 forbids
trading it off.

## What the code does

**Neither path is implemented.** There is no temp file, no rename, no `fsync`/`sync_all`,
and no journal-intent record anywhere in `pdf-write` or the coordinator.

### `DocumentCoordinator::save_incremental` (`coordinator/src/document.rs`)

```rust
let original_bytes = std::fs::read(&self.doc_path)?;
let mut file = std::fs::File::create(output_path)?;   // truncates the destination NOW
file.write_all(&original_bytes)?;                     // re-writes the whole original
IncrementalWriter::write_incremental(&mut file, …)?;  // then appends the increment
self.saved_revision = self.overlay.revision();
self.delete_sidecar();                                // drops the recovery journal
```

1. **`File::create` truncates the destination before any replacement content exists.** When
   the destination is the open document — the ordinary in-place save — the user's file is
   destroyed at that instant and rebuilt from a copy held only in memory. A crash, power
   loss, or write error between that call and the end of `write_incremental` leaves a
   truncated or half-written document with no original and no temp file to fall back to.
   That is unbounded loss, not "≤ durability budget".
2. **Nothing is fsynced.** Even the success path returns before the bytes are durable, so a
   power loss shortly after a reported-successful save can still lose the write. §3.4 names
   fsync explicitly.
3. **The sidecar is deleted immediately after the write, before any durability barrier.**
   The sidecar journal is precisely what would replay those edits after a crash (§10.3). If
   the machine dies after the unlink but before the file data reaches disk, the recovery
   record is already gone. The ordering is inverted: the recovery point must outlive the
   thing it protects.

### `ffi_bridge::save_document_impl` (`ffi-bridge/src/lib.rs`)

```rust
let mut out = original.clone();
IncrementalWriter::write_incremental(&mut out, …)?;
std::fs::write(out_path, &out)?;                      // create + truncate + write
```

Better in one respect — the whole output is assembled in memory first, so the vulnerable
window is the write itself rather than the write *plus* the increment computation. Still
truncate-in-place, still no temp file, still no fsync, still no rename.

## Why this is not caught today

`fault_inject_torn_append_truncates_to_valid` hand-builds a torn file and checks the
*scanner* tolerates it. Nothing exercises a torn write produced by our own writer, because
the writer has no mechanism that could be interrupted and rolled back.

The same test also carried an escape hatch until this change: an `Err` from the scan was
accepted as "failed gracefully — acceptable", with **no assertion in that arm**. SDS §10.5
does not offer that alternative — a document that will not open is not a valid revision — and
the arm would have swallowed any error at all, so a regression could pass silently. That one
assertion is tightened by this change; the `Ok` arm is the one taken today, so no behaviour
moves.

`docs/milestone-exit-tracker.md` records **M3 | Incremental save + recovery | Met**, and W-2
makes M3 the gate for every editing feature.

## Proposed fix, following SDS §3.4 rather than inventing one

1. **Rename path, default.** Write to a temp file in the destination directory, `sync_all`
   it, then `std::fs::rename` over the destination. Same-directory rename is atomic on NTFS,
   ext4, and APFS. Fall back only where rename is unavailable.
2. **fsync the directory** after the rename where the platform requires it for the entry to
   be durable.
3. **Delete the sidecar only after** the rename and its sync have returned. The recovery
   journal must outlive the write it protects.
4. **Append path, second slice.** Journal-intent record → append → commit, with open-time
   detection that truncates back to the last valid xref via the `/Prev` chain. Only needed
   for locked or network destinations; deferring it is fine, claiming it is not.
5. **GR-5.** Rewriting the entire original on every save is not what "untouched bytes are
   never rewritten" describes. Whether the rename path should copy-then-append or write only
   the increment is a design question this note does not settle.

## Tests this fix owes (T-5)

- Torn write produced by our own writer: kill mid-save, assert the destination still opens
  as a valid revision and that the sidecar survived to replay the difference.
- Crash after write, before rename: assert the original is untouched.
- Crash after rename, before sidecar deletion: assert replay is idempotent.
- Assert `≤ durability budget` loss explicitly rather than by implication (MET-REL-3).

## Scope note on ownership

The `ffi-bridge` half is human-gated: AI-6 and AGENTS §7 list FFI/bridge among the paths an
agent may draft but not land unreviewed. The coordinator half is not security-critical, but
it is mutation core under W-2, so it needs the fault-injection stratum above before it can
claim anything.

---

*No save-path code is modified by this change.*
