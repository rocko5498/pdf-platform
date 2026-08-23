# The shell composites its own layout; SDS §6's render pipeline has no caller

**Status:** Findings recorded; the reconciliation is an architectural decision and
needs the owner. Nothing in this note has been implemented.
**Date:** 2026-08-23
**Cites:** SDS §2.2.4, SDS §6, SDS §6.9, ADR-007, ADR-010, ADR-011, FR-NAV-1,
MET-PERF-3, GR-7, PRIN-6, T-13

---

## What is there

Two page-layout implementations exist, and the product uses the smaller one.

**One — `render_pipeline::layout` + `coordinator::render::RenderLoop`.** Page
geometries, `PagePositioner` (single, continuous, facing, continuous-facing),
`ViewportState`, viewport decomposition into tile requests, a `RenderScheduler`,
velocity-aware prefetch margins, and a `TileCache` bounded in bytes. This is what
`SDS §6` describes and what `ADR-007`, `ADR-011` and `GR-7` are written against.

**Two — `shell/canvas/canvas.cc::renderVisibleTiles`.** A single column: for each
page, `y = page_index * (page_height * scale + 16)`; for each visible tile, a
direct `render_tile()` across the bridge; the result blitted into one composite
QImage. No cache, no prefetch, no scheduler, no layout modes.

The shell uses **two**. `render_tile_impl` in the bridge goes straight to the
worker session; it never touches `RenderLoop`.

## How it was found

`grep -rn "RenderLoop" core/` returns matches only inside the module that
defines it. Its own tests exercise it; nothing else does. That is the same shape
as `reconstruct_xref`, which sat implemented and uncalled from M0 until
2026-08-23, and as the OCR job's dead dispatch arm before that.

An earlier version of this note claimed `PagePositioner` had no callers at all.
That was wrong — `RenderLoop` uses it — and the error came from a `grep` whose
output was truncated by `head`. The corrected claim is narrower and worse: the
positioner is used, by a loop nothing runs.

## What follows from it

- **`FR-NAV-1` is unmet.** It requires single, continuous, *and facing/spread*
  layouts. Facing exists, is tested, and cannot be reached from the application:
  no code path sets `PageLayout::Facing`, and the shell's compositor has no
  concept of a spread.
- **`SDS §6.9` prefetch never runs.** Neither does the scale bucketing or the
  velocity-aware margin.
- **`GR-7`'s bounded tile cache never runs.** The shell keeps one composite
  image and re-requests tiles on every scroll; the bounded `TileCache` is in the
  loop nobody calls. This is not unbounded growth — it is the opposite, no cache
  at all — but the bound the guardrail asks for is not the one in force.
- **`MET-PERF-3` measures the wrong thing.** The gate table already labels
  `viewport_scroll_2000p_us` "CPU micro, not full raster", which is honest as far
  as it goes. The deeper point is that it times `Viewport::decompose`, which the
  scrolling user's frames never execute.

## What has not been decided

Which implementation is canonical. Both readings are defensible:

**A. The coordinator owns layout (what the SDS says).** The shell asks for
visible regions and blits what it is given. Restores prefetch, the bounded cache
and the layout modes; makes `MET-PERF-3` measurable on the real path. Cost: the
canvas is rewritten around a pull API, and the bridge grows the calls to carry
viewport state and regions across it — which is `AI-6` territory.

**B. The shell owns layout (what the code does).** Then `SDS §6`, `ADR-007` and
`ADR-011` describe a component the product does not use, and the honest move is
to delete `RenderLoop` and the layout engine, amend those documents, and
re-state `MET-PERF-3` against the C++ compositor. Cheaper, and it gives up
facing layouts, prefetch and the bounded cache — three things the PRD and the
SDS ask for.

I have not chosen. **A** is what the documents require and **B** is what the
code has been doing since M1; picking between them changes an ADR either way,
which `IG §14` makes the owner's decision rather than an agent's.

## What was done instead

The tracker rows that read "Implemented" for multi-page/zoom/scroll now say
which implementation is meant and what it leaves out, per `T-13`. No code
changed.
