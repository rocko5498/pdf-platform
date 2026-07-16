// Sole C++ counterpart to the cxx bridge. [ADR-004, GR-3]
// Two-reviewer rule: any change here requires one FFI-surface owner. [ADR-027]
//
// Generated headers (rust/cxx.h, ffi-bridge/src/lib.rs.h) are produced by
// Cargo/cxx-build and placed in FFI_BRIDGE_INCLUDE_DIR.

#pragma once

#include "ffi-bridge/src/lib.rs.h"  // cxx-generated header

#include <string>

namespace pdf_platform {

/// Open a document (optional password; empty string if none). [FR-VIEW]
inline OpenResultFFI open_document(const std::string& path, const std::string& password = {}) {
    return open_document_impl(path, password);
}

/// Render a tile and return its descriptor (offset, len, generation).
inline TileResultFFI render_tile(uint32_t page, uint32_t x, uint32_t y, uint32_t w, uint32_t h,
                                 float scale, uint64_t generation) {
    return render_tile_impl(page, x, y, w, h, scale, generation);
}

inline void close_document() { close_document_impl(); }

inline uint32_t page_count() { return page_count_impl(); }

inline std::string diagnostics() { return std::string(diagnostics_impl()); }

inline std::string leniency_events() { return std::string(leniency_events_impl()); }

inline std::string get_outline() { return std::string(get_outline_impl()); }

inline std::string get_layers() { return std::string(get_layers_impl()); }

inline std::string get_attachments() { return std::string(get_attachments_impl()); }

inline std::string extract_page_text(uint32_t page) {
    return std::string(extract_page_text_impl(page));
}

inline std::string find_text(const std::string& query) {
    return std::string(find_text_impl(query));
}

inline uint64_t add_annotation(uint32_t page, const std::string& type, float x, float y, float w,
                               float h, const std::string& contents) {
    return add_annotation_impl(page, type, x, y, w, h, contents);
}

inline std::string export_xfdf() { return std::string(export_xfdf_impl()); }

inline uint32_t import_xfdf(const std::string& xml) { return import_xfdf_impl(xml); }

inline uint32_t annotation_count() { return annotation_count_impl(); }

inline std::string save_document(const std::string& out_path) {
    return std::string(save_document_impl(out_path));
}

}  // namespace pdf_platform
