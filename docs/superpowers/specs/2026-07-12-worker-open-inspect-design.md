# Design: Brokered open + worker structural inspect (M0 slice 4)

**Date:** 2026-07-12  
**Milestone:** M0 — walking skeleton  
**Depends on:** slices 1–3 (transport, spawn, WorkerSession death detect) on `main`  
**Cites:** ADR-008, ADR-010, ADR-011, ADR-016 (broker privilege), ADR-022, ADR-025,
SDS §3.1 steps 2–6, §4.2, §12 Z0/Z1, §14 M0, FR-DIAG-2  

**Prior art:** Z0 synchronous `coordinator::inspect` + CLI already scan via `pdf-cos`
**without** a worker. This slice adds the **multi-process open path** for the same
summary shape.

---

## Goal (plain language)

Prove SDS open-path **topology** end-to-end (still no PDFium tiles):

1. **Z0 Broker** validates/opens the document path (privilege stays in Z0).
2. **Spawn** a worker (existing slice 2).
3. Worker **memory-maps** the file and runs the existing **structural scan** (`pdf-cos`).
4. Worker returns a **`StructuralSummary`** over the control channel.
5. Session/API surfaces that summary to callers (CLI can optionally switch later).

## Why this step (do not skip)

| Skip | Risk |
|------|------|
| Design | Path vs handle confusion; duplicate inspect APIs forever |
| Broker | Worker opens arbitrary paths → zone leak becomes habit |
| Worker scan | M0 “open” never exercises Z1 |
| Frame codec for summary | Ad-hoc bytes only in one test |

## Scope

### In

1. **`coordinator::broker`** — `open_read_only(path) -> Result<BrokeredFile, …>`  
   validates existence + opens read-only `File` (and keeps path for M0 handoff).
2. **`pdf-cos`** — `scan_file(&File)` (or equivalent) so scan does not require
   re-open by path when we later inherit FDs.
3. **Frame body codec** for `StructuralSummary` (std-only, versioned text or similar)
   in `protocol` (next to `inspect` types).
4. **Worker** — when a document is attached, handle request frame `b"inspect"` →
   respond with encoded summary; keep echo/`quit` behavior for prior tests.
5. **`WorkerSession` (or thin helper)** — `open_and_inspect(worker_exe, path) -> StructuralSummary`
   using broker + spawn + inspect request.
6. **Tests** — real fixture PDF from `tools/corpus-diff/fixtures/valid-1page.pdf`
   (or copy path relative to workspace).

### Out

| Item | Why |
|------|-----|
| True handle inherit (no path in Z1) | Needs FD/HANDLE inherit plumbing; **documented debt** |
| Encryption / password prompt | M1+ |
| Full COS / compressed xref | Existing M0 scan limits |
| PDFium / tiles / shmem | Later M0 |
| Confinement | Human-gated |
| Shell / cxx | Later |
| Switching CLI off Z0 inspect | Optional follow-up; not required for success |

## Zone honesty (important)

SDS: *raw path never reaches Z1; only the handle does.*

**M0 decision (temporary, must be labeled):**

- Broker **does** open/validate in Z0.
- Worker still receives **`PDF_PLATFORM_DOC_PATH`** and opens/mmaps by path.

This is a **known GR-1 / SDS deviation** for this slice only, tracked as:

```text
// ponytail: DOC_PATH in Z1 is temporary; replace with inherited FD/HANDLE
// before confinement lands (slice: handle-inherit).
```

Success of *this* slice is multi-process scan + summary, not perfect zone purity.
The design forbids claiming “path never reaches worker” in PR text.

## Message contract (control channel)

Still opaque frames. M0 inspect protocol:

| Direction | Body | Meaning |
|-----------|------|---------|
| Parent → worker | `inspect` | Run structural scan on attached doc |
| Worker → parent | `SUMMARY\nv1\n…` | Encoded `StructuralSummary` |
| Parent → worker | `quit` | Clean exit (existing) |
| Parent → worker | other | Echo (existing slice 2) |

Codec (`protocol::inspect` helpers):

- Version line `v1`
- Key=value lines for scalar fields
- Zero or more `leniency=<text>` lines (text must not contain raw newlines; escape or reject)

Invalid summary decode → error to caller (honest failure).

## Attachment model

```text
BrokeredFile { path: PathBuf, file: File }  // file kept for future FD inherit

spawn_worker_with_doc(exe, &BrokeredFile) 
  → sets PDF_PLATFORM_DOC_PATH
  → spawn_worker(exe)
  → WorkerSession-like handle
```

Worker startup:

1. `adopt_inherited()` IPC
2. Read `PDF_PLATFORM_DOC_PATH` if present (optional — ping-only workers omit it)
3. Loop: recv frames; `inspect` requires doc path; else error frame or disconnect

## API sketch

```text
// coordinator::broker
pub struct BrokeredFile { /* path + File */ }
pub fn open_read_only(path: &Path) -> io::Result<BrokeredFile>;

// coordinator::session (extend)
impl WorkerSession {
    pub fn spawn_with_document(worker_exe, brokered: &BrokeredFile) -> Result<Self, SessionError>;
    pub fn inspect(&mut self) -> Result<StructuralSummary, SessionError>;
}

// protocol::inspect
pub fn encode_summary(s: &StructuralSummary) -> Vec<u8>;
pub fn decode_summary(body: &[u8]) -> Result<StructuralSummary, DecodeError>;
```

## Testing

1. Unit: encode/decode summary round-trip.
2. Unit: `scan_file` matches `scan_structure` on fixture (if both public).
3. Integration: broker open fixture → spawn_with_document → inspect → `page_count >= 1`
   for `valid-1page.pdf`.
4. Prior tests (ping, death) still green — worker without doc path unchanged.

## Success criteria

- [ ] Design + plan exist before code merges.
- [ ] Multi-process inspect returns real summary for fixture PDF on CI (3 OS).
- [ ] Broker API exists (Z0 open/validate).
- [ ] Deviation (path in Z1) documented in code + PR.
- [ ] No PDFium, confinement, shmem, Qt.
- [ ] No new crates.io deps (memmap2 already in pdf-cos).

## Next slices

1. **Handle inherit** — drop `DOC_PATH` from Z1; pass FD/HANDLE only.
2. **§10.1 respawn + re-open** using brokered handle re-transfer.
3. Confinement draft (human-gated).
4. Shmem + tile + engine.

---

*Design only until plan tasks execute.*
