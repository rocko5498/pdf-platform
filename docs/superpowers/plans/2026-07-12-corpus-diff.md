# tools/corpus-diff (v1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `tools/corpus-diff`, a structural differential-testing harness that
compares our `pdf-cos`/`coordinator` scan output against the `qpdf` oracle over a small
corpus, satisfying the SDS §14 M0 exit criterion that the corpus-diff harness exists.

**Architecture:** A new crate added to the existing `core/` Cargo workspace (no second
workspace). `src/lib.rs` holds the testable logic (qpdf detection, page-count parsing,
comparison); `src/main.rs` is a thin CLI that scans `fixtures/` and reports PASS/FAIL
with a CI-gating exit code. Malformed test data lives under `tests/fixtures/`, never in
the gating `fixtures/` directory.

**Tech Stack:** Rust (edition 2021, workspace MSRV 1.80), `std::process::Command` to
shell out to `qpdf`, no new crates.io dependency — only a path-dependency on the
existing `coordinator` crate.

## Global Constraints

- Single Cargo workspace: `corpus-diff` is added to `core/Cargo.toml`'s `members`, not
  a new `[workspace]` — this repo has no root-level workspace, only `core/`.
- No new crates.io dependency (spec: "No new dependency without flagging it").
- `qpdf` is an external process only, never linked (ADR-028).
- v1 compares **page count only** — no AcroForm/XFA/JS/signature cross-checks (spec:
  qpdf's semantics for those aren't mapped to ours anywhere).
- `fixtures/` is the gating corpus and must contain **valid files only** — a harness
  that's permanently red is not a gate. Malformed test data lives in `tests/fixtures/`.
- qpdf's exit code is not a reliable success signal (verified: exit 3 = warnings but
  valid stdout page count; exit 2 = real failure, empty stdout). Parse stdout for an
  integer regardless of exit status.
- `qpdf` v12.3.2 is installed on this machine at
  `C:\Program Files\qpdf 12.3.2\bin\qpdf.exe`, added to the **user** PATH. Any shell
  session opened before this install (including one already running in this tool)
  will not see it on PATH — prefix commands with
  `PATH="/c/Program Files/qpdf 12.3.2/bin:$PATH"` (bash) or open a fresh terminal.
- Cites: SDS §14, ADR-022, ADR-005, ADR-024, ADR-028. Design:
  `docs/superpowers/specs/2026-07-12-corpus-diff-design.md`.

---

### Task 1: Crate scaffold

**Files:**
- Create: `tools/corpus-diff/Cargo.toml`
- Create: `tools/corpus-diff/src/lib.rs`
- Create: `tools/corpus-diff/src/main.rs`
- Modify: `core/Cargo.toml`

**Interfaces:**
- Produces: crate `corpus-diff` (lib target `corpus_diff`, bin target `corpus-diff`),
  buildable via `cargo build -p corpus-diff` from `core/`.

- [ ] **Step 1: Create the crate manifest**

`tools/corpus-diff/Cargo.toml`:

```toml
[package]
name        = "corpus-diff"
description = "Structural differential-testing harness: our scanner vs the qpdf oracle. [ADR-022, SDS §14 M0 exit criteria]"
version.workspace = true
edition.workspace = true

[dependencies]
coordinator = { path = "../../core/coordinator" }
```

- [ ] **Step 2: Create empty lib and main stubs**

`tools/corpus-diff/src/lib.rs`:

```rust
// Populated in later tasks.
```

`tools/corpus-diff/src/main.rs`:

```rust
fn main() {}
```

- [ ] **Step 3: Register the crate in the workspace**

Modify `core/Cargo.toml` — add a new group after the existing `# Composition` group
(currently ends with `"cli",`):

```toml
    # Composition
    "protocol",
    "coordinator",
    "worker-main",
    "ffi-bridge",
    "cli",
    # Dev/CI tooling
    "../tools/corpus-diff",
]
```

(Only the `]` line and everything above it already exists — insert the two new lines
`# Dev/CI tooling` and `"../tools/corpus-diff",` immediately before the closing `]`.)

- [ ] **Step 4: Build to verify the scaffold compiles**

Run: `cargo build -p corpus-diff` (from `R:/Rust Project/pdf-platform/core`)
Expected: `Compiling corpus-diff v0.1.0 (...)` then `Finished` — 0 errors.

- [ ] **Step 5: Commit**

```bash
git add tools/corpus-diff/Cargo.toml tools/corpus-diff/src/lib.rs tools/corpus-diff/src/main.rs core/Cargo.toml
git commit -m "feat(corpus-diff): scaffold crate in the core workspace"
```

---

### Task 2: Fixture PDFs

**Files:**
- Create: `tools/corpus-diff/fixtures/valid-1page.pdf`
- Create: `tools/corpus-diff/fixtures/valid-3page.pdf`
- Create: `tools/corpus-diff/tests/fixtures/malformed-xref.pdf`

**Interfaces:**
- Produces: three fixture files consumed by Tasks 3-6. `fixtures/*.pdf` are the gating
  corpus (must be valid); `tests/fixtures/malformed-xref.pdf` is test-only.

All three files are exact byte content, already verified during design against both
our CLI (`cargo run -p cli --bin pdf-platform -- <file>`) and `qpdf --show-npages`.

- [ ] **Step 1: Create `fixtures/valid-1page.pdf`**

Exact content (verified: our scanner and qpdf both report 1 page):

```
%PDF-1.4
1 0 obj
<< /Type /Catalog /Pages 2 0 R >>
endobj
2 0 obj
<< /Type /Pages /Kids [3 0 R] /Count 1 >>
endobj
3 0 obj
<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << >> >>
endobj
xref
0 4
0000000000 65535 f 
0000000009 00000 n 
0000000058 00000 n 
0000000115 00000 n 
trailer
<< /Size 4 /Root 1 0 R >>
startxref
203
%%EOF
```

(Note the xref entries end in `f ` / `n ` — a trailing space before the newline, per
PDF's fixed 20-byte xref entry format.)

- [ ] **Step 2: Create `fixtures/valid-3page.pdf`**

Exact content (verified: our scanner and qpdf both report 3 pages):

```
%PDF-1.4
1 0 obj
<< /Type /Catalog /Pages 2 0 R >>
endobj
2 0 obj
<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 >>
endobj
3 0 obj
<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << >> >>
endobj
4 0 obj
<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << >> >>
endobj
5 0 obj
<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << >> >>
endobj
xref
0 6
0000000000 65535 f 
0000000009 00000 n 
0000000058 00000 n 
0000000127 00000 n 
0000000215 00000 n 
0000000303 00000 n 
trailer
<< /Size 6 /Root 1 0 R >>
startxref
391
%%EOF
```

- [ ] **Step 3: Create `tests/fixtures/malformed-xref.pdf`**

Exact content (verified: our scanner returns `Err(scan failed: malformed xref table)`;
qpdf recovers with warnings and reports 1 page — a genuine differential case, which is
exactly why it must never be in the gating `fixtures/` directory):

```
%PDF-1.4
1 0 obj
<< /Type /Catalog /Pages 2 0 R >>
endobj
2 0 obj
<< /Type /Pages /Kids [3 0 R] /Count 1 >>
endobj
3 0 obj
<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << >> >>
endobj
xref
0 4
0000000000 65535 f
0000000009 00000 n
0000000058 00000 n
0000000115 00000 n
trailer
<< /Size 4 /Root 1 0 R >>
startxref
201
%%EOF
```

(This one's xref entries deliberately lack the trailing space and carry wrong offsets —
that malformation is what triggers `MalformedXref` in our scanner.)

- [ ] **Step 4: Verify all three fixtures manually**

Run (from `R:/Rust Project/pdf-platform/core`, with qpdf's bin dir on PATH):
```bash
PATH="/c/Program Files/qpdf 12.3.2/bin:$PATH"
cargo run -q -p cli --bin pdf-platform -- ../tools/corpus-diff/fixtures/valid-1page.pdf
qpdf --show-npages ../tools/corpus-diff/fixtures/valid-1page.pdf
cargo run -q -p cli --bin pdf-platform -- ../tools/corpus-diff/fixtures/valid-3page.pdf
qpdf --show-npages ../tools/corpus-diff/fixtures/valid-3page.pdf
cargo run -q -p cli --bin pdf-platform -- ../tools/corpus-diff/tests/fixtures/malformed-xref.pdf
qpdf --show-npages ../tools/corpus-diff/tests/fixtures/malformed-xref.pdf
```
Expected: `Pages: 1` / `1`; `Pages: 3` / `3`; `error: scan failed: malformed xref table`
(nonzero exit) / `1` (with WARNING lines on stderr, exit 3).

- [ ] **Step 5: Commit**

```bash
git add tools/corpus-diff/fixtures tools/corpus-diff/tests/fixtures
git commit -m "feat(corpus-diff): add gating and test-only fixture PDFs"
```

---

### Task 3: qpdf oracle functions

**Files:**
- Modify: `tools/corpus-diff/src/lib.rs`

**Interfaces:**
- Consumes: nothing new.
- Produces: `pub fn qpdf_available() -> bool`, `pub fn qpdf_page_count(path: &Path) -> Result<u32, String>`.

- [ ] **Step 1: Write the failing tests**

Replace `tools/corpus-diff/src/lib.rs` with:

```rust
use std::path::Path;
use std::process::Command;

/// True if the `qpdf` binary can be found and run.
pub fn qpdf_available() -> bool {
    Command::new("qpdf").arg("--version").output().is_ok()
}

/// Run `qpdf --show-npages` on `path` and parse the page count from stdout.
///
/// qpdf's exit code alone is not a reliable success signal: a recoverable file
/// exits 3 (warnings) but still prints a valid count on stdout, while a truly
/// unreadable file exits 2 with empty stdout. So this parses stdout for an
/// integer regardless of exit status, and only errors when stdout has no
/// parseable number.
pub fn qpdf_page_count(path: &Path) -> Result<u32, String> {
    let output = Command::new("qpdf")
        .arg("--show-npages")
        .arg(path)
        .output()
        .map_err(|e| format!("failed to run qpdf: {e}"))?;

    match String::from_utf8_lossy(&output.stdout).trim().parse::<u32>() {
        Ok(n) => Ok(n),
        Err(_) => Err(format!(
            "qpdf reported no page count: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures").join(name)
    }

    #[test]
    fn qpdf_page_count_matches_known_good_file() {
        if !qpdf_available() {
            eprintln!("skip: qpdf not on PATH");
            return;
        }
        assert_eq!(qpdf_page_count(&fixture("valid-1page.pdf")), Ok(1));
    }

    #[test]
    fn qpdf_page_count_matches_three_page_file() {
        if !qpdf_available() {
            eprintln!("skip: qpdf not on PATH");
            return;
        }
        assert_eq!(qpdf_page_count(&fixture("valid-3page.pdf")), Ok(3));
    }
}
```

- [ ] **Step 2: Run tests to verify they pass (this task has no separate "write minimal
  code" step — the implementation above is already minimal)**

Run (from `R:/Rust Project/pdf-platform/core`):
```bash
PATH="/c/Program Files/qpdf 12.3.2/bin:$PATH" cargo test -p corpus-diff
```
Expected: `test tests::qpdf_page_count_matches_known_good_file ... ok`,
`test tests::qpdf_page_count_matches_three_page_file ... ok`, `2 passed`.

- [ ] **Step 3: Commit**

```bash
git add tools/corpus-diff/src/lib.rs
git commit -m "feat(corpus-diff): qpdf presence check and page-count parsing"
```

---

### Task 4: Comparison logic

**Files:**
- Modify: `tools/corpus-diff/src/lib.rs`

**Interfaces:**
- Consumes: `qpdf_page_count(path: &Path) -> Result<u32, String>` (Task 3),
  `coordinator::inspect::inspect(path: &Path) -> Result<protocol::inspect::StructuralSummary, coordinator::inspect::InspectError>`
  (existing, `StructuralSummary.page_count: u32`).
- Produces: `pub enum FixtureResult { Pass { file: String, page_count: u32 }, Fail { file: String, reason: String } }`,
  `pub fn compare_fixture(path: &Path) -> FixtureResult`.

- [ ] **Step 1: Write the failing tests**

Edit `tools/corpus-diff/src/lib.rs` in two places:

1. Insert the two new `pub` items below (`FixtureResult`, `compare_fixture`)
   immediately above the existing `#[cfg(test)] mod tests {` line.
2. Insert the two new `#[test] fn` items below *inside* the existing `mod tests { ... }`
   block, alongside the two tests already there (anywhere before that block's closing
   `}`).

New pub items (go above `#[cfg(test)] mod tests {`):

```rust
/// Result of comparing our scanner's structural summary against qpdf for one file.
pub enum FixtureResult {
    Pass { file: String, page_count: u32 },
    Fail { file: String, reason: String },
}

/// Compare our scanner against qpdf for one file, on page count only (v1 scope).
pub fn compare_fixture(path: &Path) -> FixtureResult {
    let file = path
        .file_name()
        .expect("fixture path must have a file name")
        .to_string_lossy()
        .to_string();

    let ours = coordinator::inspect::inspect(path).map(|s| s.page_count);
    let theirs = qpdf_page_count(path);

    match (ours, theirs) {
        (Ok(o), Ok(q)) if o == q => FixtureResult::Pass { file, page_count: o },
        (Ok(o), Ok(q)) => FixtureResult::Fail {
            file,
            reason: format!("page count mismatch: ours={o}, qpdf={q}"),
        },
        (Err(e), Ok(q)) => FixtureResult::Fail {
            file,
            reason: format!("ours=err: {e}, qpdf={q}"),
        },
        (Ok(o), Err(e)) => FixtureResult::Fail {
            file,
            reason: format!("ours={o}, qpdf=err: {e}"),
        },
        (Err(oe), Err(qe)) => FixtureResult::Fail {
            file,
            reason: format!("ours=err: {oe}, qpdf=err: {qe}"),
        },
    }
}
```

Add inside `mod tests { ... }`, alongside the existing two tests:

```rust
    #[test]
    fn compare_fixture_passes_when_counts_agree() {
        if !qpdf_available() {
            eprintln!("skip: qpdf not on PATH");
            return;
        }
        match compare_fixture(&fixture("valid-3page.pdf")) {
            FixtureResult::Pass { page_count, .. } => assert_eq!(page_count, 3),
            FixtureResult::Fail { reason, .. } => panic!("expected Pass, got Fail: {reason}"),
        }
    }

    #[test]
    fn compare_fixture_fails_on_our_scan_error() {
        if !qpdf_available() {
            eprintln!("skip: qpdf not on PATH");
            return;
        }
        let malformed = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("malformed-xref.pdf");
        match compare_fixture(&malformed) {
            FixtureResult::Fail { reason, .. } => assert!(reason.contains("ours=err")),
            FixtureResult::Pass { .. } => panic!("expected Fail for malformed-xref.pdf"),
        }
    }
```

- [ ] **Step 2: Run tests to verify they pass**

Run (from `R:/Rust Project/pdf-platform/core`):
```bash
PATH="/c/Program Files/qpdf 12.3.2/bin:$PATH" cargo test -p corpus-diff
```
Expected: 4 tests, all `ok`.

- [ ] **Step 3: Commit**

```bash
git add tools/corpus-diff/src/lib.rs
git commit -m "feat(corpus-diff): structural comparison against qpdf"
```

---

### Task 5: CLI wiring

**Files:**
- Modify: `tools/corpus-diff/src/main.rs`

**Interfaces:**
- Consumes: `corpus_diff::qpdf_available() -> bool`, `corpus_diff::compare_fixture(path: &Path) -> corpus_diff::FixtureResult` (Tasks 3-4).
- Produces: the `corpus-diff` binary. Exit 0 = all fixtures pass, 1 = at least one fixture failed, 2 = `qpdf` not on PATH.

- [ ] **Step 1: Replace `main.rs`**

```rust
use std::path::PathBuf;
use std::process::exit;

use corpus_diff::{compare_fixture, qpdf_available, FixtureResult};

fn main() {
    if !qpdf_available() {
        eprintln!("error: qpdf not found on PATH (required as the structural oracle)");
        exit(2);
    }

    let fixtures_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&fixtures_dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", fixtures_dir.display()))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "pdf"))
        .collect();
    paths.sort();

    let mut any_fail = false;
    for path in &paths {
        match compare_fixture(path) {
            FixtureResult::Pass { file, page_count } => {
                println!("PASS  {file:<24}(pages={page_count})");
            }
            FixtureResult::Fail { file, reason } => {
                any_fail = true;
                println!("FAIL  {file:<24}({reason})");
            }
        }
    }

    let total = paths.len();
    let verdict = if any_fail { "FAILURES" } else { "all passed" };
    println!("---\n{total} fixture(s) checked, {verdict}");

    exit(i32::from(any_fail));
}
```

- [ ] **Step 2: Build and run manually to verify PASS output on the gating corpus**

Run (from `R:/Rust Project/pdf-platform/core`):
```bash
PATH="/c/Program Files/qpdf 12.3.2/bin:$PATH" cargo run -q -p corpus-diff
echo "exit=$?"
```
Expected:
```
PASS  valid-1page.pdf         (pages=1)
PASS  valid-3page.pdf         (pages=3)
---
2 fixture(s) checked, all passed
exit=0
```

- [ ] **Step 3: Verify the missing-qpdf path**

Run:
```bash
PATH="/usr/bin" cargo run -q -p corpus-diff
echo "exit=$?"
```
Expected: `error: qpdf not found on PATH (required as the structural oracle)`, `exit=2`.

- [ ] **Step 4: Commit**

```bash
git add tools/corpus-diff/src/main.rs
git commit -m "feat(corpus-diff): CLI wiring over the gating fixture corpus"
```

---

### Task 6: Integration test

**Files:**
- Create: `tools/corpus-diff/tests/integration.rs`

**Interfaces:**
- Consumes: `corpus_diff::qpdf_available`, `corpus_diff::compare_fixture`, `corpus_diff::FixtureResult` (public lib API from Tasks 3-4).
- Produces: nothing consumed by later tasks — this is the terminal verification task
  for the v1 harness.

- [ ] **Step 1: Write the integration test**

`tools/corpus-diff/tests/integration.rs`:

```rust
use std::path::PathBuf;

use corpus_diff::{compare_fixture, qpdf_available, FixtureResult};

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn test_fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures")
}

#[test]
fn known_good_fixture_passes() {
    if !qpdf_available() {
        eprintln!("skip: qpdf not on PATH");
        return;
    }
    match compare_fixture(&fixtures_dir().join("valid-1page.pdf")) {
        FixtureResult::Pass { page_count, .. } => assert_eq!(page_count, 1),
        FixtureResult::Fail { reason, .. } => panic!("expected Pass, got Fail: {reason}"),
    }
}

#[test]
fn malformed_fixture_fails() {
    if !qpdf_available() {
        eprintln!("skip: qpdf not on PATH");
        return;
    }
    match compare_fixture(&test_fixtures_dir().join("malformed-xref.pdf")) {
        FixtureResult::Fail { reason, .. } => {
            assert!(reason.contains("ours=err"), "unexpected reason: {reason}");
        }
        FixtureResult::Pass { .. } => panic!("expected Fail for malformed-xref.pdf"),
    }
}
```

- [ ] **Step 2: Run the full test suite**

Run (from `R:/Rust Project/pdf-platform/core`):
```bash
PATH="/c/Program Files/qpdf 12.3.2/bin:$PATH" cargo test -p corpus-diff
```
Expected: 4 unit tests + 2 integration tests, all `ok`, `test result: ok. 6 passed`.

- [ ] **Step 3: Run the full workspace build and test to confirm nothing else broke**

Run (from `R:/Rust Project/pdf-platform/core`):
```bash
PATH="/c/Program Files/qpdf 12.3.2/bin:$PATH" cargo build --workspace
PATH="/c/Program Files/qpdf 12.3.2/bin:$PATH" cargo test --workspace
```
Expected: 0 errors on build; all tests pass across every workspace member.

- [ ] **Step 4: Commit**

```bash
git add tools/corpus-diff/tests/integration.rs
git commit -m "test(corpus-diff): integration coverage for pass and fail paths"
```
