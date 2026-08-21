# Design: `pdf-cos` has two xref parsers and ships the weaker one

**Date:** 2026-08-03
**Milestone:** M0/M1 — document open, repair, and leniency
**Status:** Report + design. No wiring is performed here; this change adds the corrupt-file
corpus and one panic fix. IG §2.3 wants the wiring reviewed before it is done.
**Cites:** ADR-005, ADR-006, SDS §10.4, SDS §10.6, FR-VIEW-2, FR-DIAG-1, MET-FEAT-1, T-2,
T-4, GR-8, PRIN-1, PRIN-6, AI-1, AI-3

---

## The finding

`core/pdf-cos/src/xref.rs` is **entirely unreachable from product code**.

```
$ grep -rn "xref::" --include=*.rs core/ | grep -v pdf-cos/src/xref.rs
core/pdf-cos/tests/leniency_corpus.rs:127:  /// `pdf_cos::xref::reconstruct_xref` implements it …
```

The only textual match outside the module itself is a doc comment added by this change.
`scan.rs` does not import it. No other crate does. It carries seven unit tests, all
passing, which is what makes the module look alive.

What is in it:

| Item | What it does | Reachable |
|---|---|---|
| `XrefTable`, `XrefEntry` | richer table model | no |
| `parse_classic_xref` | classic table parsing | no |
| `parse_xref_stream` | **PDF 1.5+ compressed xref streams** | no |
| `reconstruct_xref` | **SDS §10.4 qpdf-style recovery** | no |

Meanwhile `scan.rs` carries its own private `parse_xref_table` returning
`Vec<XrefEntry>`, and its module header states the consequence plainly:

> Reads classic xref tables only. Compressed xref (PDF 1.5+), encryption, and linearized
> hint streams are deferred to M1.

They are not deferred. `parse_xref_stream` exists and is tested. It is simply not called.

## What this costs

**1. Modern PDFs do not open.** Cross-reference streams have been the normal encoding since
PDF 1.5 (2003) and are what any producer emitting object streams writes. A document using
one reaches `parse_xref_table`, fails the `d.starts_with(b"xref")` check, and returns
`ScanError::MalformedXref`. The support for it is in the tree.

**2. Damaged documents do not recover.** SDS §10.4 and ADR-006 place qpdf-style
reconstruction in this layer — ADR-006: *"Repair/reconstruction (qpdf-style xref rebuild,
leniency ledger recording every deviation tolerated) lives here"*. ADR-005 opens on
*"rendering fidelity against decades of malformed real-world files is the single hardest
asset to build"*. The rebuild is written and never runs.

**3. Five of seven leniency event kinds cannot fire.**

| Event | Defined in | Reachable |
|---|---|---|
| `missing-pdf-header` | `scan.rs` | yes |
| `xref-truncated` | `scan.rs` | yes |
| `xref-truncated` | `xref.rs` | no |
| `unknown-xref-type` | `xref.rs` | no |
| `xref-reconstructed` | `xref.rs` | no |
| `xref-stream-decompress-failed` | `xref.rs` | no |
| `unknown-stream-filter` | `xref.rs` | no |

The corpus added by this change can only exercise the two live ones, and that is not a
limitation of the corpus.

**4. Someone already paid for this and did not notice the cause.**
`coordinator::broker::optimize_with_verification` verifies optimizer output with `qpdf
--check` rather than our own scanner, and explains why:

> empirically, `pdf_cos::scan` doesn't yet parse the xref-stream format qpdf's
> `--object-streams=generate` output uses, which would have falsely rejected valid output
> (a separate, real `pdf-cos` gap, flagged not fixed here)

The gap was diagnosed correctly and routed around. `parse_xref_stream` was sitting in the
same crate.

## Why this change does not just wire it up

The two parsers do not share a representation: `scan.rs` works with `Vec<XrefEntry>` indexed
by object number, `xref.rs` with an `XrefTable` keyed differently, and `scan_bytes` consumes
the vector shape throughout (`fetch_object`, `xref_offsets`, the incremental-save offset
map). Replacing one with the other is a structural change to the open path that alters which
documents open and how offsets reach `IncrementalWriter`.

AI-1 and AI-3 put that behind a review rather than an agent's judgement, and IG §2.3 wants
the design agreed first. This note is that design's starting point, not the change.

## Proposed shape, for review

1. **Decide the surviving representation.** `xref.rs`'s `XrefTable` is the richer one and
   already handles both encodings; `scan.rs`'s vector is what every consumer expects today.
   Adapting `XrefTable` to expose the vector view is the smaller move.
2. **Route `scan_bytes` through `xref.rs`:** classic table → xref stream → reconstruction,
   in that order, each failure recording its leniency event before falling through.
3. **Reconstruction is the last resort, never silent.** `reconstruct_xref` already pushes
   `xref-reconstructed` as its first act; that must reach `DocumentStructure::leniency` and
   the diagnostics panel (FR-DIAG-1, GR-8).
4. **Then delete the duplicate.** Two parsers for one format is how they drift; the tests in
   `xref.rs` move with it.
5. **Corpus first.** The suite this change adds already covers the live paths. Extend it with
   an xref-stream fixture and a damaged-xref fixture as the wiring lands, and un-ignore
   `a_bogus_startxref_forces_recorded_reconstruction`, which is written and waiting.

## Tests this owes (T-2, T-4)

- A PDF 1.5 document with a cross-reference stream opens, with page count matching.
- A document whose `startxref` is unusable opens by reconstruction and reports
  `xref-reconstructed`.
- A corrupt xref stream reports `xref-stream-decompress-failed` rather than failing opaquely.
- The existing prefix and byte-corruption sweeps continue to pass against the new path — a
  wider parser is a wider untrusted-input surface (T-4, GR-1).

---

*No wiring is performed by this change.*
