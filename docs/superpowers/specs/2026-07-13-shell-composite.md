# Design: Shell Bridge Composite (M0)

**Date:** 2026-07-13
**Milestone:** M0 walking skeleton
**Citations:** ADR-003, ADR-004, ADR-007, SDS §2.1, SDS §6.4

## Problem

The render pipeline proves pixels flow: coordinator → worker → shmem → TILE_READY. But nothing appears on screen. M0 requires "tile rendered through real bridge+IPC+shmem on all 3 OSes." The shell/bridge composite is the last piece: a Qt canvas that reads shmem tiles and displays them.

## Scope (M0 minimal)

1. **C++ canvas widget** — receives shmem tile descriptors, maps the shared memory, paints pixels
2. **cxx bridge** — Rust ↔ C++ boundary: submit commands, receive events
3. **Minimal Qt app** — main window with canvas, opens a test PDF, renders page 1
4. **Integration** — worker renders via stub engine, pixels appear in the window

## Non-goals (deferred)

- Full docking, panels, menus (M1+)
- GPU texture upload (software paint for M0)
- Full protocol (typed commands via cxx)
- PDFium (stub engine for now)

## Architecture

```
┌─────────────────────────────────────────────────────┐
│  Qt Process (Z0)                                    │
│                                                     │
│  ┌──────────┐   cxx bridge   ┌──────────────────┐  │
│  │ Canvas   │◀──────────────▶│ Rust Coordinator │  │
│  │ (C++)    │  tile handles  │ (in-process)     │  │
│  └──────────┘                └────────┬─────────┘  │
│                                       │             │
└───────────────────────────────────────┼─────────────┘
                                        │ IPC + shmem
                              ┌─────────▼─────────┐
                              │ Worker (Z1)        │
                              │ stub engine        │
                              └───────────────────┘
```

For M0: coordinator is in-process with the Qt app (no separate Rust process). The worker is out-of-process per ADR-008.

## Design

### 1. C++ Canvas Widget

A `QMainWindow` with a `QWidget` subclass that:
- Receives a `TileSlotDesc` (offset, len, format) + shmem file descriptor
- Maps the shmem using `QMemoryMappedFile` or platform `MapViewOfFile`/`mmap`
- Paints the RGBA8 pixels using `QImage` + `QPainter::drawImage`

### 2. cxx Bridge (minimal)

For M0, the bridge is minimal:
- Rust side: `coordinator_open(path) -> Result<DocInfo>` — opens a doc, spawns worker, returns page count
- Rust side: `coordinator_render_tile(page, x, y, w, h, scale, gen) -> Result<TileSlotDesc>` — triggers render, returns descriptor
- Rust side: `coordinator_shmem_file() -> BorrowedFd` — returns the shmem file handle for Qt to map
- C++ side: calls these functions, passes results to canvas

### 3. Minimal Qt App

```cpp
int main(int argc, char *argv[]) {
    QApplication app(argc, argv);
    MainWindow window;
    // Open a test PDF (or use stub)
    window.openDocument("test.pdf");
    window.show();
    return app.exec();
}
```

### 4. Data Flow

1. User opens file → `coordinator_open(path)` → worker spawned, shmem created
2. Coordinator sends `render_tile` to worker → worker renders via stub engine → writes shmem
3. Coordinator receives `TILE_READY` → returns `TileSlotDesc` to Qt
4. Qt canvas maps shmem at offset → creates `QImage` from RGBA8 data → paints

## Files to create

| File | Purpose |
|------|---------|
| `shell/CMakeLists.txt` | Update to find Qt6 and link |
| `shell/bridge/CMakeLists.txt` | Bridge build config |
| `shell/bridge/bridge.h` | C++ bridge header (cxx bridge counterpart) |
| `shell/bridge/bridge.cc` | C++ bridge implementation |
| `shell/canvas/canvas.h` | Canvas widget header |
| `shell/canvas/canvas.cc` | Canvas widget implementation |
| `shell/app/main.cc` | Minimal Qt app entry point |
| `shell/app/CMakeLists.txt` | App build config |
| `core/ffi-bridge/src/lib.rs` | Rust side of cxx bridge |
| `core/ffi-bridge/Cargo.toml` | FFI bridge crate |

## Risk

- cxx crate needs to compile the C++ side — requires correct include paths and Qt headers
- Shmem mapping across the FFI boundary needs careful handle passing
- M0 can defer: use a simpler approach where the Rust process writes tiles and the Qt process reads them via a temp file or shared memory path

### M0 Simplification

Instead of full cxx, M0 can use a **file-based bridge**: Rust coordinator writes tiles to a known shmem path, Qt app polls for TILE_READY files and displays them. This proves the composite works without cxx complexity.

Actually, the better M0 approach is: **embed the Rust coordinator in the Qt process** using cxx for the thin bridge, and have the worker out-of-process. The cxx bridge is the single FFI boundary (ADR-004).
