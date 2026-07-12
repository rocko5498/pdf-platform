# Design: tools/corpus-diff (v1, structural)

**Date:** 2026-07-12
**Milestone:** M0 — walking skeleton
**Cites:** SDS §14 M0 exit criteria, ADR-022 (Testing Philosophy, strata 2-3), ADR-005
(qpdf as structural correctness reference), ADR-024 (repository layout, corpus repo),
ADR-028 (dependency/license law)

---

## Goal

Build a differential-testing harness that compares our `pdf-cos` scanner's structural
output against an external oracle (`qpdf`) over a small corpus, satisfying the M0 exit
criterion that "the corpus-diff harness ... exist[s] and gate[s] this slice" (SDS §14).

## Why not pixel diff (ADR-022's actual end state)

ADR-022 strata 2-3 define corpus-diff's mature form as perceptual image-diff against
goldens plus differential rendering against oracles (hayro, MuPDF, Acrobat). That
requires an actual rasterizer producing pixels — sub-project B (tile pipeline: sandboxed
worker → shmem → GPU composite) — which does not exist yet. Building a pixel-diff
harness with no renderer behind it would be scaffolding with no real check running, which
this project's engineering discipline (spec-grounded, no unverified claims) rejects.
v1 does the smallest real, valuable differential check available today: structural
comparison, using the same `coordinator::inspect` path the M0 CLI already exercises.

## Scope

**In:** page-count differential check between our scanner and `qpdf`, over a small
checked-in fixture corpus, CI-gateable exit code.

**Out (deferred):**
- Pixel/image-diff against render oracles — needs sub-project B.
- AcroForm/XFA/JS/signature-count cross-checks against qpdf — qpdf's detection
  semantics for these fields aren't mapped to ours anywhere in the spec; guessing at
  the mapping now would invent behavior. Revisit once a field-equivalence decision is
  made.
- The real corpus repository (ADR-024: separate, LFS-backed, its own licensing
  governance) — not provisioned. v1 ships a handful of small, permissively-licensed
  fixture PDFs directly in the tool's own directory as an explicit placeholder.
- CI wiring — arrives with the pipeline work in sub-project B or D.
- Triage UI (ADR-022's diff-triage tooling commitment) — applies to image-diff, not
  this structural v1.

## Architecture

New crate: `tools/corpus-diff` — directory placement follows ADR-024's top-level layout
(`tools/` holds "triage UI, corpus tooling, benchmark harness"), but it is added as a
**member of the existing `core/Cargo.toml` workspace** via a relative path
(`"../tools/corpus-diff"`), not a second independent workspace. There is no root-level
Cargo workspace in this repo (only `core/`), and `core/cli` is already nested the same
way despite ADR-024's literal top-level `cli/` wording — a second workspace here would
mean two `Cargo.lock`s that can drift, doubled compile/CI time, and no real benefit.
One workspace, one lockfile. It is a dev/CI utility, not a shipped product component.

```
tools/
  corpus-diff/
    Cargo.toml            # workspace member, not its own [workspace]
    src/
      lib.rs               # qpdf invocation + comparison logic (testable)
      main.rs               # thin CLI: scans fixtures/, prints results, sets exit code
    fixtures/               # the GATING corpus — valid files only, must always PASS
      valid-1page.pdf
      valid-3page.pdf
    tests/
      fixtures/             # TEST-ONLY fixtures, never scanned by the gating binary
        malformed-xref.pdf
      integration.rs
```

**Why malformed fixtures are not in `fixtures/`.** `fixtures/` is what the shipped
binary scans to gate CI. A deliberately-broken file living there would make the
harness report FAIL permanently, on every run, regardless of whether anything is
actually wrong — a gate that's always red is not a gate. The malformed case still
needs to exist (to prove `compare_fixture` correctly detects and reports a mismatch),
so it lives under `tests/fixtures/`, read only by the test suite, never by the binary's
own fixture scan.

**Dependencies.** Path-deps on `coordinator` and `protocol` (siblings in the same
workspace — reuse `coordinator::inspect`, the exact function the CLI calls, rather than
reimplementing scan logic). No new crates.io dependency.

**Oracle.** Shells out to the `qpdf` binary via `std::process::Command` — external
process, never linked (ADR-028's posture for AGPL/external-oracle tools: qpdf itself is
Apache-2.0/Artistic and fine to link, but treating it as an external CLI oracle here
matches how ADR-022 describes MuPDF/Acrobat oracle usage — invoked, not embedded).
Command: `qpdf --show-npages <file>`. `qpdf` must be on PATH (documented as a
prerequisite, not vendored); confirmed installed and working (v12.3.2) during design.

qpdf's exit code is **not** a reliable success signal on its own: on a recoverable file
it exits `3` (warnings) but still prints a valid page count to stdout, while a truly
unreadable file exits `2` with empty stdout (verified against both cases). The oracle
call therefore parses stdout for an integer regardless of exit status, and only reports
an oracle error when stdout has no parseable number.

**Comparison (v1, one field).** For each fixture:
1. Run `coordinator::inspect(path)` → `StructuralSummary.page_count`.
2. Run `qpdf --show-npages <path>`, parse the integer stdout.
3. PASS if equal, FAIL if not, FAIL if either side errors (missing file, qpdf crash,
   scan error) — with the reason printed.

**Output.** One line per fixture to stdout:
```
PASS  valid-1page.pdf       (ours=1, qpdf=1)
FAIL  malformed-xref.pdf    (ours=err: malformed xref table, qpdf=1)
```
Summary line + exit code: `0` if all pass, `1` if any fixture fails, `2` if `qpdf` is
not found on PATH (environment problem, distinct from a real comparison failure — a CI
gate should not silently pass just because the oracle binary is missing, and should not
be confused with an actual regression either).

## Error handling

A malformed or unreadable fixture is reported as FAIL for that file, never a panic or
crash (consistent with FR-VIEW-2 / the project's leniency philosophy — even test
harness code degrades gracefully). Missing `qpdf` binary is checked once, up front,
before iterating fixtures, so a broken environment fails fast with one clear message
instead of N confusing per-file errors.

## Testing

Integration test (`tests/integration.rs`) covering both outcomes: `tests/fixtures/`'s
known-good file asserts PASS, `tests/fixtures/malformed-xref.pdf` asserts FAIL is
reported with a reason mentioning the mismatch. Both are skipped with a clear message
if `qpdf` is not found on PATH in the test environment — an external tool dependency is
not something CI/dev setup should silently fake or a test should hard-fail on when it's
simply absent.

## What this does NOT implement

- Pixel/perceptual image-diff → after sub-project B
- Render-oracle differential testing (hayro/MuPDF/Acrobat) → after sub-project B
- AcroForm/XFA/JS/signature structural cross-checks → needs a qpdf-field-mapping
  decision first
- The real ADR-024 corpus repository → separate provisioning effort
- CI pipeline wiring → sub-project B or D

## Implementation order

1. `tools/corpus-diff` crate scaffold (Cargo.toml, added to `core/Cargo.toml`'s
   `members` via relative path — same workspace, same lockfile)
2. Gating fixtures (`fixtures/valid-1page.pdf`, `fixtures/valid-3page.pdf`) and the
   test-only malformed fixture (`tests/fixtures/malformed-xref.pdf`)
3. `qpdf` presence check + `--show-npages` invocation and stdout-based parsing
4. Comparison loop (`compare_fixture`) + PASS/FAIL output + exit codes
5. Integration tests (skip-if-missing-qpdf)
