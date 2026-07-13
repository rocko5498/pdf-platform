# Design: Rasterize Tile Pipeline (M0 slice)

**Date:** 2026-07-13
**Milestone:** M0 walking skeleton
**Citations:** ADR-005, ADR-007, ADR-008, SDS §6, SDS §2.3, GR-4, GR-7

## Problem

PR #9 proved shmem plumbing works (smoke fill). But zero PDF content flows through the pipeline. We need the full render path: coordinator sends a render request → worker rasterizes → pixels appear in shmem → coordinator reads them back.

PDFium prebuilt is not yet available (`third_party/pdfium/prebuilt/` is empty). So we build the pipeline with a **stub engine** that produces colored test patterns. This proves the plumbing end-to-end; PDFium swaps in behind the same trait later.

## Scope (this slice)

1. **engine-api** — define `Rasterize` trait with request/response types
2. **engine-stub** — new crate, stub engine producing colored test patterns per page
3. **worker-main** — add `render_tile` handler: deserialize request → call engine → write pixels into shmem slot → send `TILE_READY`
4. **protocol** — add `RenderTileRequest` text protocol + encode/decode
5. **render-pipeline::shmem** — multi-slot shmem pool (allocate slots, track generations)
6. **Integration test** — coordinator creates pool, spawns worker, sends `render_tile`, reads real pixels back

## Non-goals (explicit)

- PDFium FFI (separate slice when prebuilt is pinned)
- Viewport decomposition / tile scheduling / cache (M0.5/M1)
- Shell/GPU composite (separate slice)
- Confinement (human-gated)

## Design

### 1. Rasterize trait (`engine-api/src/rasterize.rs`)

```rust
/// Request to rasterize a page region.
pub struct RasterizeRequest {
    pub page_index: u32,
    /// Device-space rect within the page (x, y, w, h in pixels).
    pub rect: TileRect,
    /// Output scale (1.0 = 72 DPI device pixels).
    pub scale: f32,
}

pub struct TileRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// Output pixel buffer.
pub struct TileOutput {
    pub rgba_pixels: Vec<u8>,  // RGBA8, tightly packed
    pub width: u32,
    pub height: u32,
}

pub trait Rasterize: Send + Sync {
    fn rasterize(&self, req: &RasterizeRequest) -> Result<TileOutput, RasterizeError>;
    fn page_count(&self) -> u32;
}

pub enum RasterizeError {
    PageOutOfRange(u32),
    EngineError(String),
}
```

### 2. Stub engine (`core/engine-stub/`)

New crate. Implements `Rasterize`. Produces a colored checkerboard pattern per page:
- Each page gets a distinct hue (page 0 = red, page 1 = green, etc.)
- Checkerboard squares of 32px
- This proves: page_index is correct, rect is correct, scale is applied, pixels are real

### 3. Worker render_tile handler

New text protocol message:
```
render_tile
v1
page=0
x=0
y=0
w=256
h=256
scale=1.0
generation=1
```

Worker handler:
1. Parse request
2. Call `engine.rasterize(req)`
3. Write RGBA8 pixels into shmem at offset 0 (single-slot for M0)
4. Send `TILE_READY` with the slot descriptor

### 4. Multi-slot shmem pool (`render-pipeline/src/shmem.rs`)

```rust
pub struct TilePool {
    region: SharedRegion,
    slot_size: usize,      // TILE_RGBA8_BYTES
    slots: Vec<SlotState>,  // generation tracking
}

struct SlotState {
    generation: u64,
    allocated: bool,
}

impl TilePool {
    pub fn create(num_slots: usize) -> Result<Self>;
    pub fn alloc_slot(&mut self, generation: u64) -> Option<(u32, usize)>; // (offset, slot_index)
    pub fn slot_slice(&self, slot_index: usize) -> &[u8];
    pub fn slot_slice_mut(&mut self, slot_index: usize) -> &mut [u8];
    pub fn invalidate.Generation(&mut self, generation: u64);
}
```

For M0: single slot is sufficient. Pool is designed for future multi-tile.

### 5. Integration test

```rust
#[test]
fn render_tile_end_to_end() {
    // 1. Create TilePool with 1 slot
    // 2. Spawn worker with shmem attached
    // 3. Send "render_tile\nv1\npage=0\nx=0\ny=0\nw=256\nh=256\nscale=1.0\ngeneration=1"
    // 4. Receive TILE_READY
    // 5. Decode TileSlotDesc
    // 6. Read pixels from pool at offset
    // 7. Assert: pixels are not all zero, first pixel matches expected color
}
```

## Files to create/modify

| File | Action |
|------|--------|
| `core/engine-api/src/rasterize.rs` | Define Rasterize trait + types |
| `core/engine-stub/Cargo.toml` | New crate |
| `core/engine-stub/src/lib.rs` | Stub engine impl |
| `core/protocol/src/commands.rs` | RenderTileRequest encode/decode |
| `core/render-pipeline/src/shmem.rs` | TilePool |
| `core/worker-main/src/main.rs` | Add render_tile handler |
| `core/worker-main/Cargo.toml` | Add engine-stub dep |
| `core/Cargo.toml` (workspace) | Add engine-stub member |

## Risk

- The stub engine doesn't prove PDFium works, but it proves the pipeline works. When PDFium prebuilt is pinned, we swap `engine-stub` for `engine-pdfium` behind the same trait.
