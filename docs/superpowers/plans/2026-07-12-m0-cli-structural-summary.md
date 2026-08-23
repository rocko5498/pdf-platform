# M0 CLI Structural Summary Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement `pdf-platform <file>` that prints page count, AcroForm/XFA/JS/sig presence, and leniency events for a PDF document.

**Architecture:** Minimal xref-aware COS scanner in `pdf-cos` reads a file without decoding streams; `coordinator` wraps it in a synchronous `inspect` function (in-process for CLI, no actor/channel needed); `cli/main.rs` formats and prints.

**Tech Stack:** Rust 2021, workspace MSRV 1.80, `memmap2` v0.9 (MIT) as the only new dep.

## Global Constraints

- Rust edition 2021, MSRV 1.80 (per workspace Cargo.toml)
- No `unsafe` without `// SAFETY:` comment (per ADR-027)
- Every change cites spec IDs (SDS §14 M0, FR-DIAG-2, US-DEV-6, ADR-006, ADR-025, ADR-010)
- No clap / arg-parsing library at M0 — use `std::env::args()` (full CLI at M6)
- No JSON output at M0 — human-readable only (JSON at M6 with FR-CLI parity)
- No async runtime in any crate (ADR-010 / GR-6)
- Compressed xref (PDF 1.5+ xref streams), encryption, linearized PDFs are out of scope — M1

---

## File Map

| File | Status | What changes |
|---|---|---|
| `core/pdf-cos/Cargo.toml` | modify | add `memmap2 = "0.9"` |
| `core/pdf-cos/src/leniency.rs` | fill stub | `LeniencyEvent` struct + `Display` |
| `core/pdf-cos/src/scan.rs` | create | `DocumentStructure`, `ScanError`, `scan_structure`, `scan_bytes`, all helpers |
| `core/pdf-cos/src/lib.rs` | modify | `pub mod leniency; pub mod scan;` |
| `core/protocol/src/inspect.rs` | create | `StructuralSummary` struct |
| `core/protocol/src/lib.rs` | modify | `pub mod inspect;` |
| `core/coordinator/src/inspect.rs` | create | `InspectError`, `inspect(path)` |
| `core/coordinator/src/lib.rs` | modify | `pub mod inspect;` |
| `core/cli/src/main.rs` | fill stub | arg parse, call coordinator, print, exit codes |

---

## Task 1: pdf-cos — LeniencyEvent, DocumentStructure, and COS scanner (TDD)

**Files:**
- Modify: `core/pdf-cos/Cargo.toml`
- Fill: `core/pdf-cos/src/leniency.rs`
- Create: `core/pdf-cos/src/scan.rs`
- Modify: `core/pdf-cos/src/lib.rs`

**Interfaces:**
- Produces: `pub fn scan_structure(path: &Path) -> Result<DocumentStructure, ScanError>` (used by Task 3)
- Produces: `pub(crate) fn scan_bytes(data: &[u8]) -> Result<DocumentStructure, ScanError>` (used by test in this task)
- Produces: `pub struct DocumentStructure { page_count: u32, has_acroform: bool, has_xfa: bool, has_js: bool, sig_count: u32, leniency: Vec<LeniencyEvent> }`
- Produces: `pub struct LeniencyEvent { kind: &'static str, detail: String }` + `impl Display`

---

- [ ] **Step 1: Add memmap2 dep to pdf-cos/Cargo.toml**

Replace the contents of `core/pdf-cos/Cargo.toml` with:

```toml
[package]
name        = "pdf-cos"
description = "COS object store, xref, filter pipeline, leniency ledger. [ADR-006, ADR-025]"
version.workspace = true
edition.workspace = true

[dependencies]
pdf-types   = { path = "../pdf-types" }
diagnostics = { path = "../diagnostics" }
memmap2     = "0.9"
```

- [ ] **Step 2: Fill in core/pdf-cos/src/leniency.rs**

```rust
//! Tolerated deviations from the PDF specification. [ADR-006, FR-DIAG-1]

/// A single tolerated parse deviation recorded during document scanning.
#[derive(Debug, Clone)]
pub struct LeniencyEvent {
    pub kind: &'static str,
    pub detail: String,
}

impl LeniencyEvent {
    pub fn new(kind: &'static str, detail: impl Into<String>) -> Self {
        Self { kind, detail: detail.into() }
    }
}

impl std::fmt::Display for LeniencyEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.kind, self.detail)
    }
}
```

- [ ] **Step 3: Write the failing test first in core/pdf-cos/src/scan.rs**

Create the file with just the test and the public types, leaving `scan_bytes` as `todo!()`:

```rust
//! Minimal structural scanner. [ADR-006, SDS §14 M0, FR-DIAG-2]
//!
//! Reads classic xref tables only. Compressed xref (PDF 1.5+), encryption,
//! and linearized hint streams are deferred to M1.

use std::path::Path;
use crate::leniency::LeniencyEvent;

/// Structural summary of a PDF document produced by the minimal M0 scanner.
#[derive(Debug)]
pub struct DocumentStructure {
    pub page_count: u32,
    pub has_acroform: bool,
    pub has_xfa: bool,
    pub has_js: bool,
    pub sig_count: u32,
    pub leniency: Vec<LeniencyEvent>,
}

/// Fatal scanner errors (not tolerable leniency events).
#[derive(Debug)]
pub enum ScanError {
    Io(std::io::Error),
    NoStartxref,
    NoTrailer,
    NoRoot,
    MalformedXref,
}

impl std::fmt::Display for ScanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScanError::Io(e)        => write!(f, "I/O error: {e}"),
            ScanError::NoStartxref  => write!(f, "no startxref marker found"),
            ScanError::NoTrailer    => write!(f, "no trailer dictionary found"),
            ScanError::NoRoot       => write!(f, "no /Root in trailer"),
            ScanError::MalformedXref => write!(f, "malformed xref table"),
        }
    }
}

impl std::error::Error for ScanError {}

impl From<std::io::Error> for ScanError {
    fn from(e: std::io::Error) -> Self { ScanError::Io(e) }
}

/// Scan a PDF file and return its structural summary.
pub fn scan_structure(path: &Path) -> Result<DocumentStructure, ScanError> {
    let file = std::fs::File::open(path)?;
    // SAFETY: read-only shared mapping; the file is not mutated while the Mmap is live.
    let map = unsafe { memmap2::Mmap::map(&file) }?;
    scan_bytes(&map)
}

/// Scan raw PDF bytes. Exposed for testing without file I/O.
pub(crate) fn scan_bytes(_data: &[u8]) -> Result<DocumentStructure, ScanError> {
    todo!("implement in Step 5")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal hand-crafted 1-page PDF with no AcroForm/JS/sigs.
    /// Byte offsets are exact — do not reformat this literal.
    /// obj1@9  obj2@56  obj3@111  xref@180  startxref=180
    const MINIMAL_PDF: &[u8] = b"\
%PDF-1.0\n\
1 0 obj\n\
<</Type /Catalog /Pages 2 0 R>>\n\
endobj\n\
2 0 obj\n\
<</Type /Pages /Kids [3 0 R] /Count 1>>\n\
endobj\n\
3 0 obj\n\
<</Type /Page /Parent 2 0 R /MediaBox [0 0 612 792]>>\n\
endobj\n\
xref\n\
0 4\n\
0000000000 65535 f \n\
0000000009 00000 n \n\
0000000056 00000 n \n\
0000000111 00000 n \n\
trailer\n\
<</Size 4 /Root 1 0 R>>\n\
startxref\n\
180\n\
%%EOF";

    #[test]
    fn scan_minimal_pdf() {
        let ds = scan_bytes(MINIMAL_PDF).expect("scan should succeed");
        assert_eq!(ds.page_count, 1);
        assert!(!ds.has_acroform);
        assert!(!ds.has_xfa);
        assert!(!ds.has_js);
        assert_eq!(ds.sig_count, 0);
        assert!(ds.leniency.is_empty());
    }
}
```

- [ ] **Step 4: Expose modules in core/pdf-cos/src/lib.rs**

The existing lib.rs has module stubs. Add or confirm these lines are present:

```rust
pub mod leniency;
pub mod scan;
// existing stubs remain:
pub mod object;
pub mod xref;
pub mod filter;
pub mod store;
```

- [ ] **Step 5: Run the test to verify it fails with todo!()**

```
cd "R:/Rust Project/pdf-platform/core"
cargo test -p pdf-cos scan_minimal_pdf 2>&1
```

Expected: compile succeeds; test panics with `not yet implemented: implement in Step 5`.

- [ ] **Step 6: Implement all helpers and complete scan_bytes**

Replace the `scan_bytes` `todo!()` stub and add the helper functions below it (before the `#[cfg(test)]` block):

```rust
pub(crate) fn scan_bytes(data: &[u8]) -> Result<DocumentStructure, ScanError> {
    let mut leniency = Vec::new();

    if !data.starts_with(b"%PDF-") {
        leniency.push(LeniencyEvent::new("missing-pdf-header", "no %PDF- marker at byte 0"));
    }

    let xref_offset = find_startxref(data).ok_or(ScanError::NoStartxref)?;
    let xref = parse_xref_table(data, xref_offset, &mut leniency)?;
    let trailer = find_trailer(data, xref_offset).ok_or(ScanError::NoTrailer)?;

    let root_ref = find_indirect_ref(trailer, b"/Root").ok_or(ScanError::NoRoot)?;
    let catalog = fetch_object(data, &xref, root_ref.0).unwrap_or(b"");

    let has_acroform = find_key(catalog, b"/AcroForm").is_some();
    let has_xfa = find_key(catalog, b"/XFA").is_some()
        || (has_acroform && fetch_key_dict(data, &xref, catalog, b"/AcroForm")
                .map(|d| find_key(d, b"/XFA").is_some())
                .unwrap_or(false));
    let has_js = find_key(catalog, b"/JS").is_some()
        || names_tree_has_javascript(data, &xref, catalog);

    let page_count = find_indirect_ref(catalog, b"/Pages")
        .and_then(|(n, _)| fetch_object(data, &xref, n))
        .and_then(|obj| parse_int_after_key(obj, b"/Count"))
        .unwrap_or(0) as u32;

    let sig_count = if has_acroform {
        count_sig_field_pattern(
            fetch_key_dict(data, &xref, catalog, b"/AcroForm").unwrap_or(b""),
        )
    } else {
        0
    };

    Ok(DocumentStructure { page_count, has_acroform, has_xfa, has_js, sig_count, leniency })
}

// --- private helpers ---

#[derive(Clone, Default)]
struct XrefEntry { offset: u64, in_use: bool }

/// Scan last 1024 bytes for `startxref\n<N>`, return N as a file offset.
fn find_startxref(data: &[u8]) -> Option<usize> {
    const NEEDLE: &[u8] = b"startxref";
    let search_start = data.len().saturating_sub(1024);
    let tail = &data[search_start..];
    // Find last occurrence of NEEDLE
    let mut last = None;
    for i in 0..=tail.len().saturating_sub(NEEDLE.len()) {
        if &tail[i..i + NEEDLE.len()] == NEEDLE { last = Some(i); }
    }
    let pos = last?;
    let mut i = pos + NEEDLE.len();
    while i < tail.len() && matches!(tail[i], b' ' | b'\r' | b'\n') { i += 1; }
    let start = i;
    while i < tail.len() && tail[i].is_ascii_digit() { i += 1; }
    std::str::from_utf8(&tail[start..i]).ok()?.parse().ok()
}

/// Parse a classic (non-compressed) xref table at `offset`.
fn parse_xref_table(
    data: &[u8],
    offset: usize,
    leniency: &mut Vec<LeniencyEvent>,
) -> Result<Vec<XrefEntry>, ScanError> {
    let d = data.get(offset..).ok_or(ScanError::MalformedXref)?;
    if !d.starts_with(b"xref") {
        // Likely a compressed xref stream (PDF 1.5+) — not supported at M0.
        return Err(ScanError::MalformedXref);
    }
    let mut pos = 4; // after "xref"
    skip_eol(d, &mut pos);

    let mut entries: Vec<XrefEntry> = Vec::new();

    loop {
        // Check for trailer keyword — marks end of xref sections.
        if d.get(pos..).map_or(false, |s| s.starts_with(b"trailer")) { break; }

        let first = match parse_uint(d, &mut pos) { Some(n) => n, None => break };
        skip_ws(d, &mut pos);
        let count = match parse_uint(d, &mut pos) { Some(n) => n, None => break };
        skip_eol(d, &mut pos);

        let needed = first + count;
        if entries.len() < needed { entries.resize(needed, XrefEntry::default()); }

        for obj in first..first + count {
            if pos + 20 > d.len() {
                leniency.push(LeniencyEvent::new("xref-truncated", "xref table ends early"));
                break;
            }
            let entry_bytes = &d[pos..pos + 20];
            // Format: "OOOOOOOOOO GGGGG N/F \n" — byte 17 is 'n' or 'f'
            let offset_bytes = &entry_bytes[0..10];
            let in_use = entry_bytes.get(17) == Some(&b'n');
            let byte_offset = std::str::from_utf8(offset_bytes)
                .ok()
                .and_then(|s| s.trim().parse::<u64>().ok())
                .unwrap_or(0);
            entries[obj] = XrefEntry { offset: byte_offset, in_use };
            pos += 20;
        }
    }
    Ok(entries)
}

/// Return the slice starting at the trailer dictionary `<<...`.
fn find_trailer<'a>(data: &'a [u8], xref_offset: usize) -> Option<&'a [u8]> {
    let region = data.get(xref_offset..)?;
    let tpos = region.windows(7).position(|w| w == b"trailer")?;
    let after = tpos + 7;
    let dict_start = after + region[after..].iter().position(|&b| b == b'<')?;
    Some(&region[dict_start..])
}

/// Fetch the body of an indirect object by number (between obj/endobj).
fn fetch_object<'a>(data: &'a [u8], xref: &[XrefEntry], num: u32) -> Option<&'a [u8]> {
    let entry = xref.get(num as usize)?;
    if !entry.in_use { return None; }
    let d = data.get(entry.offset as usize..)?;
    // Skip "N G obj" header + whitespace
    let body_start = d.windows(4).position(|w| w == b" obj").map(|p| {
        let after = p + 4;
        after + d[after..].iter().position(|&b| !matches!(b, b' ' | b'\t' | b'\r' | b'\n')).unwrap_or(0)
    })?;
    let body = &d[body_start..];
    let end = body.windows(6).position(|w| w == b"endobj").unwrap_or(body.len());
    Some(&body[..end])
}

/// Find the first occurrence of `key` bytes in `data`. Returns the position of the key start.
fn find_key(data: &[u8], key: &[u8]) -> Option<usize> {
    data.windows(key.len()).position(|w| w == key)
}

/// Find `/Key N G R` in `data` and return (N, G).
fn find_indirect_ref(data: &[u8], key: &[u8]) -> Option<(u32, u16)> {
    let pos = find_key(data, key)?;
    let after = &data[pos + key.len()..];
    let mut i = 0;
    skip_ws(after, &mut i);
    let obj_num = parse_uint(after, &mut i)? as u32;
    skip_ws(after, &mut i);
    let gen_num = parse_uint(after, &mut i)? as u16;
    skip_ws(after, &mut i);
    if after.get(i) == Some(&b'R') { Some((obj_num, gen_num)) } else { None }
}

/// Follow an indirect ref from a named key and return the target object body.
fn fetch_key_dict<'a>(
    data: &'a [u8],
    xref: &[XrefEntry],
    parent: &[u8],
    key: &[u8],
) -> Option<&'a [u8]> {
    let (n, _) = find_indirect_ref(parent, key)?;
    fetch_object(data, xref, n)
}

/// Parse `/Key <integer>` and return the integer value.
fn parse_int_after_key(data: &[u8], key: &[u8]) -> Option<i64> {
    let pos = find_key(data, key)?;
    let after = &data[pos + key.len()..];
    let mut i = 0;
    skip_ws(after, &mut i);
    let neg = after.get(i) == Some(&b'-');
    if neg { i += 1; }
    let start = i;
    while i < after.len() && after[i].is_ascii_digit() { i += 1; }
    if i == start { return None; }
    let n: i64 = std::str::from_utf8(&after[start..i]).ok()?.parse().ok()?;
    Some(if neg { -n } else { n })
}

/// Check if the /Names tree in the catalog has a /JavaScript entry.
fn names_tree_has_javascript(data: &[u8], xref: &[XrefEntry], catalog: &[u8]) -> bool {
    fetch_key_dict(data, xref, catalog, b"/Names")
        .map(|names| find_key(names, b"/JavaScript").is_some())
        .unwrap_or(false)
}

/// Count occurrences of `/FT /Sig` pattern (proxy for sig fields at M0 scope).
fn count_sig_field_pattern(acroform_body: &[u8]) -> u32 {
    // ponytail: pattern match instead of full field-tree walk; sufficient for M0 simple PDFs
    const SIG: &[u8] = b"/FT /Sig";
    let mut count = 0u32;
    let mut i = 0;
    while i + SIG.len() <= acroform_body.len() {
        if &acroform_body[i..i + SIG.len()] == SIG { count += 1; i += SIG.len(); } else { i += 1; }
    }
    count
}

fn parse_uint(data: &[u8], pos: &mut usize) -> Option<usize> {
    let start = *pos;
    while *pos < data.len() && data[*pos].is_ascii_digit() { *pos += 1; }
    if *pos == start { return None; }
    std::str::from_utf8(&data[start..*pos]).ok()?.parse().ok()
}

fn skip_ws(data: &[u8], pos: &mut usize) {
    while *pos < data.len() && matches!(data[*pos], b' ' | b'\t' | b'\r' | b'\n') { *pos += 1; }
}

fn skip_eol(data: &[u8], pos: &mut usize) {
    if data.get(*pos) == Some(&b'\r') { *pos += 1; }
    if data.get(*pos) == Some(&b'\n') { *pos += 1; }
}
```

- [ ] **Step 7: Run the test — verify it passes**

```
cargo test -p pdf-cos scan_minimal_pdf 2>&1
```

Expected output:
```
test scan::tests::scan_minimal_pdf ... ok
```

- [ ] **Step 8: Commit**

```bash
git add core/pdf-cos/Cargo.toml core/pdf-cos/src/leniency.rs core/pdf-cos/src/scan.rs core/pdf-cos/src/lib.rs
git commit -m "feat(pdf-cos): minimal COS scanner for M0 structural summary

Implements scan_structure + scan_bytes with classic xref table parse,
page-tree count, and catalog flag detection (AcroForm/XFA/JS/Sig).
LeniencyEvent type records tolerated parse deviations.
One unit test with embedded minimal-PDF byte literal.

Cites: SDS §14 M0, FR-DIAG-2, ADR-006, ADR-025
"
```

---

## Task 2: protocol — StructuralSummary type

**Files:**
- Create: `core/protocol/src/inspect.rs`
- Modify: `core/protocol/src/lib.rs`

**Interfaces:**
- Consumes: nothing from earlier tasks (pure data type)
- Produces: `pub struct StructuralSummary { page_count: u32, has_acroform: bool, has_xfa: bool, has_js: bool, sig_count: u32, leniency_count: u32, leniency_events: Vec<String> }` (used by Tasks 3 and 4)

---

- [ ] **Step 1: Create core/protocol/src/inspect.rs**

```rust
//! Inspect command result type. [ADR-025, FR-DIAG-2]

/// Structural summary of a PDF document, returned by the inspect command.
/// Wire type owned by protocol; pdf-cos owns the raw parse result.
#[derive(Debug, Clone)]
pub struct StructuralSummary {
    pub page_count: u32,
    pub has_acroform: bool,
    pub has_xfa: bool,
    pub has_js: bool,
    pub sig_count: u32,
    pub leniency_count: u32,
    /// Human-readable leniency event descriptions (M0: strings; M6: structured).
    pub leniency_events: Vec<String>,
}
```

- [ ] **Step 2: Add to core/protocol/src/lib.rs**

Add `pub mod inspect;` alongside the existing module stubs. Do not remove existing stubs.

- [ ] **Step 3: Build to verify**

```
cargo build -p protocol 2>&1
```

Expected: `Compiling protocol` ... no errors.

- [ ] **Step 4: Commit**

```bash
git add core/protocol/src/inspect.rs core/protocol/src/lib.rs
git commit -m "feat(protocol): add StructuralSummary type for inspect command

Wire type for the M0 CLI inspect path. Kept separate from pdf-cos
DocumentStructure so protocol can add serialization later without
touching the parse layer.

Cites: ADR-025, FR-DIAG-2
"
```

---

## Task 3: coordinator — inspect function

**Files:**
- Create: `core/coordinator/src/inspect.rs`
- Modify: `core/coordinator/src/lib.rs`

**Interfaces:**
- Consumes: `pdf_cos::scan::scan_structure(path) -> Result<DocumentStructure, ScanError>`
- Consumes: `protocol::inspect::StructuralSummary`
- Produces: `pub fn inspect(path: &std::path::Path) -> Result<protocol::inspect::StructuralSummary, InspectError>` (used by Task 4)
- Produces: `pub enum InspectError` with `Display + Error`

---

- [ ] **Step 1: Create core/coordinator/src/inspect.rs**

```rust
//! Synchronous document inspect command. [ADR-010, ADR-025, FR-DIAG-2]
//!
//! The CLI links coordinator in-process; this path is synchronous and does not
//! use the actor/channel model (which applies to the document open/render lifecycle).

use std::path::Path;
use pdf_cos::scan::{scan_structure, ScanError};
use protocol::inspect::StructuralSummary;

/// Error returned by [`inspect`].
#[derive(Debug)]
pub enum InspectError {
    Scan(ScanError),
}

impl std::fmt::Display for InspectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InspectError::Scan(e) => write!(f, "scan failed: {e}"),
        }
    }
}

impl std::error::Error for InspectError {}

/// Inspect a PDF file and return its structural summary.
///
/// Synchronous — no worker spawn, no shmem, no sandbox for this path.
pub fn inspect(path: &Path) -> Result<StructuralSummary, InspectError> {
    let ds = scan_structure(path).map_err(InspectError::Scan)?;
    Ok(StructuralSummary {
        page_count:      ds.page_count,
        has_acroform:    ds.has_acroform,
        has_xfa:         ds.has_xfa,
        has_js:          ds.has_js,
        sig_count:       ds.sig_count,
        leniency_count:  ds.leniency.len() as u32,
        leniency_events: ds.leniency.iter().map(|e| e.to_string()).collect(),
    })
}
```

- [ ] **Step 2: Add to core/coordinator/src/lib.rs**

Add `pub mod inspect;` alongside existing stubs.

- [ ] **Step 3: Build to verify**

```
cargo build -p coordinator 2>&1
```

Expected: no errors. (Other stub modules may produce `missing_docs` warnings — that is acceptable.)

- [ ] **Step 4: Commit**

```bash
git add core/coordinator/src/inspect.rs core/coordinator/src/lib.rs
git commit -m "feat(coordinator): add synchronous inspect fn for CLI path

Wraps pdf-cos scan_structure and maps to protocol::StructuralSummary.
No actor/channel — CLI is in-process; this is a diagnostic probe, not
a document lifecycle open.

Cites: ADR-010, ADR-025, FR-DIAG-2, SDS §14 M0
"
```

---

## Task 4: cli — main.rs wiring and output

**Files:**
- Fill: `core/cli/src/main.rs`

**Interfaces:**
- Consumes: `coordinator::inspect::inspect(path) -> Result<StructuralSummary, InspectError>`
- Consumes: `protocol::inspect::StructuralSummary`
- Produces: `pdf-platform <file>` binary that exits 0/1/2

---

- [ ] **Step 1: Fill core/cli/src/main.rs**

```rust
//! `pdf-platform` CLI entry point. [ADR-025, FR-CLI, US-DEV-6, SDS §14 M0]
//!
//! M0 scope: structural summary only. Full CLI surface at M6.

use std::{path::PathBuf, process};

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() != 2 {
        eprintln!("usage: pdf-platform <file>");
        process::exit(1);
    }

    let path = PathBuf::from(&args[1]);

    if !path.exists() {
        eprintln!("error: not found: {}", path.display());
        process::exit(1);
    }

    match coordinator::inspect::inspect(&path) {
        Ok(s) => {
            println!("Pages:      {}", s.page_count);
            println!("AcroForm:   {}", yn(s.has_acroform));
            println!("XFA:        {}", yn(s.has_xfa));
            println!("JavaScript: {}", yn(s.has_js));
            println!("Signatures: {}", s.sig_count);
            if s.leniency_count == 0 {
                println!("Leniency:   0 repairs");
            } else {
                println!("Leniency:   {} repair(s) — details on stderr", s.leniency_count);
                for event in &s.leniency_events {
                    eprintln!("  leniency: {event}");
                }
            }
            process::exit(0);
        }
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(2);
        }
    }
}

fn yn(b: bool) -> &'static str { if b { "yes" } else { "no" } }
```

- [ ] **Step 2: Build the binary**

```
cargo build -p cli 2>&1
```

Expected: `Compiling cli` ... `Finished`. Binary at `target/debug/pdf-platform`.

- [ ] **Step 3: Smoke-test with a real PDF**

Find any PDF on the machine (e.g., a downloaded document):

```
./target/debug/pdf-platform path/to/some.pdf
```

Expected output (values will vary):
```
Pages:      N
AcroForm:   yes/no
XFA:        no
JavaScript: no
Signatures: 0
Leniency:   0 repairs
```

Exit code should be 0. Verify: `echo $?` (bash) or `$LASTEXITCODE` (PowerShell).

- [ ] **Step 4: Test the error path**

```
./target/debug/pdf-platform nonexistent.pdf
```

Expected: prints `error: not found: nonexistent.pdf`, exit code 1.

- [ ] **Step 5: Commit**

```bash
git add core/cli/src/main.rs
git commit -m "feat(cli): implement pdf-platform structural summary command

Satisfies M0 exit criterion: pdf-platform <file> prints page count,
AcroForm/XFA/JS/sig presence, and leniency events. Exit codes: 0 ok,
1 bad arg/not-found, 2 fatal scan error.

Cites: SDS §14 M0, FR-DIAG-2, US-DEV-6, ADR-025, FR-CLI
"
```

---

## Self-Review

**Spec coverage check:**
- ✅ `pdf-platform <file>` prints page count → Task 4 Step 1 (`page_count`)
- ✅ AcroForm/XFA/JS/sig presence → Task 1 Step 6 (`has_acroform`, `has_xfa`, `has_js`, `sig_count`)
- ✅ Leniency ledger → Task 1 Step 2 (`LeniencyEvent`) + Task 4 Step 1 (stderr output)
- ✅ Human-readable output → Task 4 Step 1
- ✅ Exit codes 0/1/2 → Task 4 Step 1
- ✅ memmap2 dep added → Task 1 Step 1
- ✅ SAFETY comment on unsafe block → Task 1 Step 3
- ✅ No async runtime → confirmed: all paths are synchronous
- ✅ No clap → `std::env::args()` only
- ✅ Spec IDs cited in every commit → confirmed

**Placeholder scan:** None found.

**Type consistency check:**
- `DocumentStructure` defined in Task 1, consumed in Task 3 ✅
- `StructuralSummary` defined in Task 2, consumed in Tasks 3 and 4 ✅
- `InspectError` defined and used in Task 3 ✅
- `inspect(path: &Path) -> Result<StructuralSummary, InspectError>` matches call in Task 4 ✅
- `scan_structure(path: &Path) -> Result<DocumentStructure, ScanError>` matches call in Task 3 ✅
