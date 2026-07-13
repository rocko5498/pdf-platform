// Sole C++ counterpart to the cxx bridge. [ADR-004, GR-3]
// Two-reviewer rule: any change here requires one FFI-surface owner. [ADR-027]
//
// Generated headers (rust/cxx.h, ffi-bridge/src/lib.rs.h) are produced by
// Cargo/cxx-build and placed in FFI_BRIDGE_INCLUDE_DIR.

#pragma once

#include "ffi-bridge/src/lib.rs.h"  // cxx-generated header

namespace pdf_platform {

/// Open a document via the Rust coordinator. Returns page count and shmem handle.
inline OpenResultFFI open_document(const std::string& path) {
    return open_document_impl(path);
}

/// Render a tile and return its descriptor (offset, len, generation).
inline TileResultFFI render_tile(uint32_t page, uint32_t x, uint32_t y,
                                  uint32_t w, uint32_t h, float scale,
                                  uint64_t generation) {
    return render_tile_impl(page, x, y, w, h, scale, generation);
}

/// Close the current document and kill the worker.
inline void close_document() {
    close_document_impl();
}

}  // namespace pdf_platform
