# Design: Benchmarks Baseline (M0)

**Date:** 2026-07-13
**Milestone:** M0 walking skeleton
**Citations:** ADR-023, SDS §14, MET-PERF-*, MET-GOV-1

## Problem

ADR-023 requires two benchmark tiers: micro (criterion-class) and macro (scripted end-to-end). M0 needs the micro tier established with the render pipeline as the first subject. This proves the measurement infrastructure works and establishes baseline numbers for future regression gating.

## Scope (this slice)

1. **Criterion benchmarks** for the render pipeline:
   - Stub engine rasterization throughput (tiles/sec, MB/sec)
   - TilePool allocation latency
   - IPC round-trip for render_tile (command → response)
   - Codec roundtrip for RenderTileRequest encode/decode

2. **Benchmark harness** in `core/benches/` as a separate binary crate

## Non-goals

- Macro scenario benchmarks (need Qt shell + reference documents)
- Dedicated hardware gating (CI integration)
- Reference document corpus (not yet available)

## Design

### Criterion binary crate

`core/benches/render_pipeline.rs` — criterion benchmarks for the render pipeline.

### Benchmarks

1. **`bench_rasterize_256x256`** — stub engine renders a 256×256 tile. Measures:
   - Throughput (tiles/sec)
   - Latency (ns/iter)
   
2. **`bench_rasterize_various_sizes`** — parameterized: 64×64, 128×128, 256×256, 512×512

3. **`bench_tile_pool_alloc`** — allocate + mark ready from a 4-slot pool

4. **`bench_render_tile_codec`** — encode + decode RenderTileRequest

5. **`bench_ipc_render_tile_roundtrip`** — cross-process: spawn worker, send render_tile, receive TILE_READY. This is the most important M0 benchmark — it measures the full pipeline latency.

### Output

Criterion generates HTML reports in `target/criterion/`. Baseline numbers recorded for comparison.

## Files to create/modify

| File | Action |
|------|--------|
| `core/Cargo.toml` | Add `bench` target |
| `core/Cargo.toml` | Add `criterion` dev-dependency |
| `core/benches/render_pipeline.rs` | New benchmark file |
