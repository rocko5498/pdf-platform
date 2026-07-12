# Design: M0 CLI Structural Summary

**Date:** 2026-07-12
**Milestone:** M0 — walking skeleton
**Cites:** SDS §14 M0 exit criteria, FR-DIAG-2, US-DEV-6, ADR-006, ADR-025, ADR-010

---

## Goal

Implement `pdf-platform <file>` such that it prints a structural summary of a PDF
document: page count, presence of AcroForm / XFA / JavaScript / signatures, and any
leniency events (repairs made during parse). This satisfies one of the four M0 exit
criteria and exercises the foundational pdf-cos parse path.

## Scope

**In:** classic xref table + trailer parse, page tree page count, catalog flag detection
(AcroForm, XFA, JS, Sig), leniency event collection, human-readable stdout output.

**Out (deferred to M1):** compressed xref streams (PDF 1.5+), encrypted file open,
linearized hint streams, full COS filter pipeline, JSON output (comes with FR-CLI
full parity at M6).

## Architecture

Four crates are modified; no new crates added.

### 1. `pdf-cos` — scan_structure

Add `pub fn scan_structure(path: &Path) -> Result<DocumentStructure, CosError>`.

Steps:
1. `memmap2::Mmap` the file (read-only, shared). New dep: `memmap2` (MIT, no network,
   no unsafe beyond the mmap syscall itself — standard practice).
2. Find `startxref` by scanning backwards from `%%EOF`.
3. Parse the classic cross-reference table at that offset.
4. Parse the trailer dictionary.
5. Lazy-fetch only the objects we need via the xref table:
   - Catalog dict → check for `/AcroForm`, `/Names`→`/JavaScript`, `/XFA`, `/Perms`,
     `/MarkInfo` (skip — not needed for summary).
   - Page tree root → walk `/Kids` counting leaf pages (only the count; no page content).
   - `/AcroForm` dict → check for `/SigFlags` and count `/Fields` with `/FT /Sig`.
6. Collect `LeniencyEvent` for every tolerated parse deviation (missing `%%PDF` header,
   malformed xref entry, trailer with missing required key, etc.) via the existing
   `diagnostics` crate leniency API.
7. Return `DocumentStructure`.

```rust
pub struct DocumentStructure {
    pub page_count: u32,
    pub has_acroform: bool,
    pub has_xfa: bool,
    pub has_js: bool,
    pub sig_count: u32,
    pub leniency: Vec<LeniencyEvent>,
}
```

Object parsing uses a minimal hand-rolled COS token scanner (not a full parser):
enough to read dict keys, integer values, and indirect object refs. The full COS
parser is M1 work.

### 2. `protocol` — StructuralSummary

Add `StructuralSummary` as a plain data struct mirroring `DocumentStructure`. Protocol
crate owns the wire shape; pdf-cos owns the parse result. These are kept separate so
the protocol type can evolve independently (e.g., add JSON serialization later without
touching pdf-cos).

```rust
pub struct StructuralSummary {
    pub page_count: u32,
    pub has_acroform: bool,
    pub has_xfa: bool,
    pub has_js: bool,
    pub sig_count: u32,
    pub leniency_count: u32,
    pub leniency_events: Vec<String>,  // human-readable for M0
}
```

### 3. `coordinator` — inspect

Add `pub fn inspect(path: &Path) -> Result<StructuralSummary, InspectError>`.

Synchronous (no actor/channel for the CLI inspect path — the CLI process links the
coordinator in-process, not via IPC). Calls `pdf_cos::scan_structure`, maps result to
`StructuralSummary`. No worker spawn, no shmem, no sandbox for this path.

Per ADR-010: the actor/channel model applies to the document lifecycle (open → render →
edit → close). The inspect path is a diagnostic read-only probe that does not open a
document in the lifecycle sense.

### 4. `cli/main.rs` — argument parsing and output

Minimal arg parsing: `std::env::args()` — no clap for M0 (YAGNI; full CLI surface at M6).

```
pdf-platform <file>         # print summary to stdout, exit 0
pdf-platform <file> 2>log   # leniency events on stderr
```

Output format:
```
Pages:      42
AcroForm:   yes
XFA:        no
JavaScript: yes
Signatures: 2
Leniency:   3 repairs (see stderr)
```

Exit codes: 0 = success, 1 = file not found / unreadable, 2 = fatal parse error (not
merely leniency-repaired).

## New dependency

`memmap2` v0.9 (MIT). Maps to `mmap(2)` on Linux/macOS, `MapViewOfFile` on Windows.
No network, no unsafe beyond the syscall boundary. Single well-maintained crate.
Exit seam: if removed, replace with `std::fs::read` (slower for large files, same
semantics for M0 "simple PDF" targets).

## Test

One `#[test]` in `pdf-cos` that embeds a minimal hand-crafted PDF byte string (the
smallest valid 1-page PDF is ~200 bytes) and asserts:
- `page_count == 1`
- `has_acroform == false`
- `has_js == false`
- `leniency.is_empty()`

No test fixtures, no file I/O in test — the minimal PDF is a byte literal in the test.

## What this does NOT implement

- Compressed xref (PDF 1.5+ `/XRef` stream objects) → M1
- Encrypted file open → M1
- Linearization hint stream → M1
- Full COS object graph / filter pipeline → M1+
- JSON output → M6 (FR-CLI full parity)
- `--help`, `--version` → M6

## Implementation order

1. `pdf-types`: add `LeniencyEvent` if not already present in diagnostics
2. `pdf-cos`: `scan_structure` + minimal COS tokenizer + test
3. `protocol`: `StructuralSummary` type
4. `coordinator`: `inspect` fn
5. `cli`: `main.rs` wiring + output format
