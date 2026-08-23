# ADR-039 — Tests Must Assert Evidence, Not Activity

**Status:** Proposed — requires human ratification (`AI-6` does not apply; this is process,
not a security-critical path, but it amends a binding ADR so §14 requires an owner's decision)
**Date:** 2026-08-23
**Amends:** ADR-022 (testing strategy)
**Cites:** ADR-022, ADR-023, PRIN-1, PRIN-6, GR-8, T-1..T-9, AI-2, AI-7, AI-8, G-1,
MET-GOV-2, SDS §12, IG §8

---

## Context

Between M0 and M11 this project accumulated a green CI, a milestone tracker that read
"complete" for eleven milestones, and roughly two dozen defects that no test could have
caught. None of them were regressions. Every one shipped broken and was recorded as
working. A representative sample, all found by reading code rather than by any suite:

- **OCR had never recognised a single character.** `ocr-bridge`'s `deflate_store` emitted
  raw deflate blocks with no zlib header, so every OCR image stream was undecodable. The
  test asserted that the OCR path was reached and that a PDF came back.
- **Every object fetched from the worker ran from its offset to end of file.**
  `handle_get_object` scanned with `windows(7)` for the six-byte needle `endobj`, so the
  match never occurred and the fallback returned the remainder of the document. The test
  asserted the returned bytes contained the object header.
- **Page rotation had never rotated anything.** The caller passed page object numbers, the
  builder matched page indices, so the rotation applied to zero pages. Had it matched, the
  command would have overwritten each page with `<< /Rotate 90 >>` and destroyed its
  content. The test built a command group in memory and asserted the group was non-empty.
- **The GPU tile test accepted `RenderError` as a pass.** It asserted the call returned,
  not that a tile came back.
- **The plugin fuel-limit test executed no WebAssembly.** It constructed a limits struct
  and asserted its fields held the values just assigned to them.

The common shape is not carelessness; it is a test written from the implementation's point
of view. It asks "did my code run?" when the requirement asks "did the document change?".
A test that asks the first question passes for every implementation that reaches the line,
including the ones that then do nothing, do the opposite, or destroy data.

The second shape is two layers agreeing on a name and disagreeing on its meaning — indices
vs. object numbers, page-relative vs. document-relative offsets, dpi assumed vs. dpi given.
Each layer's unit tests passed, because each layer was self-consistent. Nothing exercised
the seam.

The third shape is a status claim recorded when the code was written rather than when it
was observed working. Four separate surfaces — the tracker, a session note, a milestone
summary and a set of PR descriptions — reported success for work that had never run.

## Decision

Amend `ADR-022` with four rules. They are additive; the existing strata `T-1..T-9` are
unchanged and still apply.

### EV-1 — Assert the observable outcome, never the activity

A test asserts a property of the artefact the requirement talks about: the saved document,
the rendered pixels, the extracted text, the returned tile. It does not assert that a
function was called, that a result was `Ok`, that a struct holds the value just written to
it, or that a byte string appears somewhere in a buffer.

The rule of thumb: **if the test would still pass against an implementation whose body is
deleted down to a plausible return value, it is asserting activity.** Delete the body and
check. This is cheap and it is the single highest-yield check in this document.

`Ok(())` is not an outcome. "Did not panic" is not an outcome. "Contains the substring" is
an outcome only when the substring's absence is the defect being guarded.

### EV-2 — Every gate must have been observed failing at least once

A gate — a test, a CI job, an assertion, a metric threshold — that has never failed is
indistinguishable from a gate that cannot fail. Before a gate is trusted, it must be made
to fail once against a deliberately wrong input or a deliberately broken implementation,
and that observation is recorded in the PR that adds it.

This is what would have caught all five defects above at the moment they were introduced:
each test passes against the broken code, and also passes against no code at all.

Recording the observation is one line ("removed the zlib header, test failed with
`invalid distance too far back`; restored, passes"). It is not a separate mutation-testing
apparatus and does not require one.

### EV-3 — Every layer seam carries one end-to-end test

Wherever two components exchange a value whose meaning could be read two ways — an index,
an offset, an object number, a scale, a unit, a coordinate origin — one test drives the
real path across the seam and asserts the outcome on the far side. Unit tests on both sides
of a seam do not test the seam; they test each side's belief about it.

For this codebase the seams are: shell ↔ FFI bridge, bridge ↔ coordinator, coordinator ↔
worker (IPC), worker ↔ engine, and engine ↔ external tool (Tesseract, qpdf, veraPDF).

### EV-4 — A status claim cites its evidence or says "not measured"

Any document that records progress — the milestone exit tracker, release gates, ADR status
lines, PR descriptions — states, for each claim, the command that was run and what it
printed, or marks the claim **not measured**. "Implemented" is not a status. A budget
without a measurement on pinned reference hardware (`B-3`, `MET-GOV-1`) is **not measured**,
and saying so is a passing state; asserting it silently is not.

`AI-8` already forbids fabricating results. EV-4 is narrower and mechanical: absent
evidence, the honest string is available and required.

## Consequences

- `IG §8` gains `T-10..T-13` restating these rules where the PR checklist points.
- `PR-6` additionally requires the EV-2 failure observation for each new gate.
- Existing tests are not retroactively rewritten in bulk. When a test is touched, or when
  the code it covers is touched, it is brought to EV-1. A sweep would produce a large diff
  with no failing test to justify any individual line of it.
- This slows a green build down. That is the intent: `PRIN-1` puts correctness before
  capability, and eleven milestones of green CI over broken code is the cost of the
  alternative.

## Alternatives considered

**Mutation testing (`cargo-mutants`) instead of EV-2.** Would mechanise the check, and is a
plausible later addition. Rejected as the primary rule because it is a CI-time tool for a
discipline problem: it reports surviving mutants long after the author has moved on, it is
slow on a 26-crate workspace, and it does not address EV-3 or EV-4 at all. EV-2 costs one
minute at the moment of writing, when the author still has the context to act on it.

**Coverage thresholds.** Rejected: every one of the defects above sat on a covered line.
Coverage measures execution, which is precisely the thing EV-1 says not to assert.

**Do nothing; rely on `AI-7` and `PRIN-6`.** `AI-7` forbids tests that assert buggy
behaviour and `PRIN-6` requires honesty. Both were in force throughout, and neither
prevented any of this, because neither tells an author what a sufficient assertion looks
like. The gap was operational, not ethical.
