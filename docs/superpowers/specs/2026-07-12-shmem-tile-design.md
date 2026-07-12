# Design: Shared-memory tile buffer seam (M0 slice 7)

**Date:** 2026-07-12  
**Milestone:** M0  
**Depends on:** slices 1–6 on `main` (through PR #8)  
**Cites:** ADR-004, ADR-007, ADR-011 (GR-7 bound), ADR-022, ADR-025, ADR-027, ADR-028,
SDS §4.2 bulk tiles, §6.2–6.3, §14 M0 (tile via IPC+shmem path)

---

## Goal

Land a **cross-process shared-memory buffer** that:

1. Parent (Z0) **creates** a fixed-size region (one 256×256 RGBA tile = 256 KiB).
2. Region is **inherited** into the worker as FD/HANDLE (same pattern as document file).
3. Worker **writes** a deterministic smoke pattern (no PDFium yet).
4. Worker signals readiness over the control channel (`TILE_READY` frame).
5. Parent **reads** the pattern from its mapping and validates.

This proves SDS “bulk tiles via shmem + control via IPC” without engine or GPU.

## Why before PDFium

M0 exit needs tile through bridge+IPC+shmem. Engine binding is separate risk;
shmem inherit + descriptor validation is its own footgun. Isolate it.

## Scope

### In

| Piece | Detail |
|-------|--------|
| `protocol::handles` | `PixelFormat`, `TileSlotDesc` (offset, len, format, generation) |
| `protocol` codec | `encode_tile_ready` / `decode_tile_ready` text v1 |
| `sandbox::shmem` | `SharedRegion::create(len)`, mmap via **memmap2 0.9** (already in workspace via pdf-cos; pin same; MIT, ADR-028 note: exit = swap mmap backend) |
| Spawn | Optional second inherit: `PDF_PLATFORM_SHMEM_FD` / `_HANDLE` |
| Worker | Adopt shmem; on `tile_smoke` fill pattern; reply `TILE_READY` |
| Session / test | Create region, spawn with doc+shmem or shmem-only, smoke test |
| Bound | Single slot, fixed size; GR-7 documented |

### Out

| Item | Why |
|------|-----|
| PDFium rasterize | Next slice |
| Multi-slot pool / MemoryGovernor | Later |
| Shell GPU upload / cxx | Later |
| Prefetch / generation cancel full policy | Stub generation field only |

## Sizing

SDS §6.2: **256×256** logical px baseline.  
M0 smoke: **RGBA8** → `256 * 256 * 4 = 262_144` bytes per slot.  
Pool: **1** slot (max 256 KiB resident). Bound explicit in code.

## Smoke pattern

Worker writes:

- Bytes `[0..8)` = `b"PDFSHMEM"`
- Remaining bytes = `0xA5`

Parent asserts after `TILE_READY`.

## API sketch

```text
// sandbox::shmem
pub struct SharedRegion { /* File + MmapMut + len */ }
impl SharedRegion {
    pub fn create(len: usize) -> io::Result<Self>;
    pub fn len(&self) -> usize;
    pub fn file(&self) -> &File;
    pub fn as_slice(&self) -> &[u8];
    // parent may need as_mut for zeroing; worker maps separately
}

// spawn
spawn_worker_with_attachments(exe, doc: Option<&File>, shmem: Option<&File>)
ENV_SHMEM_FD / ENV_SHMEM_HANDLE
adopt_shmem_file() -> Option<File>

// protocol::handles
pub const TILE_EDGE_PX: u32 = 256;
pub const TILE_RGBA8_BYTES: usize = 256*256*4;
pub enum PixelFormat { Rgba8 }
pub struct TileSlotDesc { offset, len, format, generation }

// worker frame
parent -> worker: b"tile_smoke"
worker -> parent: encode_tile_ready(TileSlotDesc { offset:0, len: TILE_RGBA8_BYTES, ...})
```

## Safety

- `MmapMut::map_mut` SAFETY: exclusive mapping of region we own; size matches `set_len`.
- Handle inherit: same SAFETY pattern as document FD (ADR-027).

## Success criteria

- [ ] Design + plan
- [ ] Integration test: parent sees smoke pattern after worker fill (3-OS CI)
- [ ] No PDFium required
- [ ] Bound documented (1 × 256 KiB)
- [ ] memmap2 version pinned + license note in PR

## Next

Engine rasterize into slot, then shell composite (cxx/Qt), then confinement.

---

*Design only until plan executes.*
