# Design: Document handle inherit into worker (M0 slice 5)

**Date:** 2026-07-12  
**Milestone:** M0  
**Depends on:** slices 1–4 on `main` (through PR #6)  
**Cites:** ADR-008, ADR-016, ADR-027 (`unsafe` + SAFETY), ADR-028 (no new dep if avoidable),
SDS §3.1 steps 2–4, §4.2, §12 GR-1, FR-DIAG-2  

**Closes debt from:** `docs/superpowers/specs/2026-07-12-worker-open-inspect-design.md`
(`PDF_PLATFORM_DOC_PATH` in Z1).

---

## Goal

Worker structural inspect must **not** receive a filesystem path.

1. Z0 **broker** still opens the file.
2. Parent marks the OS handle/FD **inheritable**, passes only a **numeric handle/FD**
   in the environment (not a path string).
3. Worker **adopts** that handle into a `std::fs::File` and runs `pdf_cos::scan::scan_file`.
4. Remove `PDF_PLATFORM_DOC_PATH` from the spawn/inspect path.

## Why

SDS / GR-1: raw path must not reach Z1. Slice 4 accepted temporary path debt; this
slice pays it for the document file. (IPC still uses path/port connect-after-bind;
that is separate and already documented.)

## Scope

### In

| Item | Detail |
|------|--------|
| Unix | Clear `FD_CLOEXEC` on brokered `File`; env `PDF_PLATFORM_DOC_FD=<int>`; worker `File::from_raw_fd` |
| Windows | Set handle inherit flag; env `PDF_PLATFORM_DOC_HANDLE=<usize>`; worker `File::from_raw_handle` |
| Session | `spawn_with_document` uses handle inherit API, not path env |
| Worker | Adopt doc file from FD/HANDLE; `scan_file`; no path open for inspect |
| Remove | `ENV_DOC_PATH` usage for inspect (delete or leave unused deprecated constant) |
| Tests | Existing `open_inspect` still passes; assert path env **not** set in child if easy |

### Out

| Item | Why |
|------|-----|
| IPC FD inherit (replace sock path / TCP port) | Separate; works today |
| Confinement | Human-gated later |
| DuplicateHandle multi-worker fan-out | Single worker M0 |
| windows-sys / libc crates | Prefer minimal `extern "C"` / `extern "system"` + SAFETY notes (ADR-028) |

## API sketch

```text
// sandbox::spawn
pub const ENV_DOC_FD: &str = "PDF_PLATFORM_DOC_FD";       // unix
pub const ENV_DOC_HANDLE: &str = "PDF_PLATFORM_DOC_HANDLE"; // windows

pub fn spawn_worker_with_file(
    worker_exe: &Path,
    doc: &File,
    extra_env: &[(&str, &str)], // optional, usually empty
) -> io::Result<WorkerChild>;

// worker-side
pub fn adopt_document_file() -> io::Result<Option<File>>;
// None if no doc env (ping-only worker)
```

`WorkerSession::spawn_with_document` calls `spawn_worker_with_file(exe, doc.file(), &[])`.

## Safety

Every `unsafe` block needs `// SAFETY:` (ADR-027):

- Parent: fcntl / SetHandleInformation on a live FD/HANDLE owned by `BrokeredFile`.
- Child: `from_raw_fd` / `from_raw_handle` only for the value passed by parent for this process.

Parent keeps `BrokeredFile` alive across spawn so the FD remains open until the child
has its inherited copy.

## Env contract (updated)

| Variable | Role |
|----------|------|
| `PDF_PLATFORM_IPC_SOCK` / `_PORT` | IPC (unchanged) |
| `PDF_PLATFORM_DOC_FD` | Unix document FD (new) |
| `PDF_PLATFORM_DOC_HANDLE` | Windows document HANDLE as integer (new) |
| ~~`PDF_PLATFORM_DOC_PATH`~~ | **Removed** from product path |

## Testing

1. `open_inspect` fixture still returns page_count == 1.
2. Worker without doc env: `inspect` fails cleanly (exit or error); ping still works.
3. 3-OS CI.

## Success criteria

- [ ] No path string required for worker inspect.
- [ ] Fixture multi-process inspect green on CI.
- [ ] `unsafe` blocks documented.
- [ ] No new crates.io dependency.
- [ ] Design + plan before merge.

## Next

IPC channel inherit (optional), confinement draft, or tile/shmem.

---

*Design only until plan tasks execute.*
