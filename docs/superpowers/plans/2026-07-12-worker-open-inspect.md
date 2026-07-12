# Implementation Plan: Brokered open + worker inspect (M0 slice 4)

> Design: `docs/superpowers/specs/2026-07-12-worker-open-inspect-design.md`  
> Do not skip tasks. Prefer small commits.

**Branch:** `feat/m0-worker-open-inspect` from `main` (after PR #5).

**Cites:** ADR-008, ADR-011, ADR-016, ADR-022, ADR-025, SDS §3.1, FR-DIAG-2

---

### Task 0: Preconditions

- [ ] PR #5 on `main`
- [ ] Read design
- [ ] Branch + claim/log

---

### Task 1: `pdf-cos` scan from `File`

**Files:** `core/pdf-cos/src/scan.rs`

1. Add `pub fn scan_file(file: &std::fs::File) -> Result<DocumentStructure, ScanError>`
2. Keep `scan_structure(path)` as open + `scan_file`
3. Unit test if easy; else rely on integration
4. Commit: `feat(pdf-cos): scan_file for mmap from opened File`

---

### Task 2: Protocol summary frame codec

**Files:** `core/protocol/src/inspect.rs`

1. `encode_summary` / `decode_summary` (v1 text)
2. Unit round-trip test
3. Commit: `feat(protocol): StructuralSummary frame codec v1`

---

### Task 3: Broker open_read_only

**Files:** `core/coordinator/src/broker.rs`

1. `BrokeredFile { path, file }`
2. `open_read_only(path)` — metadata check + `File::open`
3. Commit: `feat(coordinator): broker open_read_only for documents`

---

### Task 4: Spawn with document env + session inspect

**Files:**  
- `core/sandbox/src/spawn.rs` (optional helper or env constant)  
- `core/coordinator/src/session.rs`  
- `core/worker-main/src/main.rs`

1. Env `PDF_PLATFORM_DOC_PATH` constant (document // ponytail zone debt)
2. `WorkerSession::spawn_with_document(exe, &BrokeredFile)`
3. `WorkerSession::inspect()` — send `inspect`, recv, decode
4. Worker: on `inspect`, scan path, reply encoded summary
5. Commit: `feat(session,worker): multi-process structural inspect`

---

### Task 5: Integration test

**Files:** `core/worker-main/tests/open_inspect.rs`

1. Path to `tools/corpus-diff/fixtures/valid-1page.pdf` via `CARGO_MANIFEST_DIR`
2. Broker → spawn_with_document → inspect → assert page_count == 1 (or >= 1)
3. Commit: `test: worker open inspect on fixture PDF`

---

### Task 6: Verify + PR

1. `cargo test -p pdf-cos -p protocol -p coordinator -p worker-main --all-targets`
2. `cargo build --workspace`
3. Open PR with zone-debt note; wait CI; merge when user asked to move on
   (this session: user said merge and move on — merge after green)

---

## Non-goals

- [ ] No FD inherit yet
- [ ] No PDFium / tiles
- [ ] No confinement
- [ ] No CLI switch required
