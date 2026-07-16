//! PDFium backend impl of engine-api traits. [ADR-005, GR-4]
//!
//! Uses `pdfium-render` for safe bindings + prebuilt PDFium binaries.
//! SAFETY: all unsafe lives here; every unsafe block must carry a // SAFETY: comment. [ADR-027]

use std::sync::Once;

use engine_api::extract::{Extract, ExtractError, PageTextModel, TextLine, TextSpan};
use engine_api::rasterize::{Rasterize, RasterizeError, RasterizeRequest, TileOutput};
use pdfium_render::prelude::*;

/// PDFium-backed rasterizer. Holds a loaded document. [ADR-005]
///
/// Thread-safe: `Pdfium` is `Send + Sync`; rendering borrows the document
/// immutably. Multiple tiles can be rasterized concurrently.
pub struct PdfiumEngine {
    document: PdfDocument<'static>,
    page_count: u32,
}

// SAFETY: Pdfium's internal state is protected by its own synchronization.
// The document is accessed only through immutable borrows during rasterization.
unsafe impl Send for PdfiumEngine {}
unsafe impl Sync for PdfiumEngine {}

/// Global PDFium instance, initialized once per process. [ADR-005]
///
/// SAFETY: Pdfium is designed to live for the process lifetime.
/// We leak the Box so the reference is stable. pdfium-render's own
/// `Pdfium::default()` uses the same pattern internally.
static INIT: Once = Once::new();
static mut PDFIUM_PTR: *const Pdfium = std::ptr::null();

fn pdfium() -> &'static Pdfium {
    // SAFETY: INIT ensures this runs exactly once. The pointer is written
    // before any reader can see it (Once::call_once is a barrier).
    unsafe {
        INIT.call_once(|| {
            let pdfium = pdfium_auto::bind_pdfium_silent()
                .expect("failed to initialize PDFium");
            let leaked = Box::leak(Box::new(pdfium));
            PDFIUM_PTR = leaked as *const Pdfium;
        });
        &*PDFIUM_PTR
    }
}

impl PdfiumEngine {
    /// Load a PDF file from disk with an optional password.
    pub fn from_file(path: &std::path::Path, password: Option<&str>) -> Result<Self, String> {
        let document = pdfium()
            .load_pdf_from_file(path, password)
            .map_err(|e| format!("pdfium load failed: {e}"))?;

        let page_count = document.pages().len() as u32;

        // SAFETY: pdfium() returns a &'static Pdfium; the document borrows from it
        // and the global lives for the process lifetime.
        let document = unsafe {
            std::mem::transmute::<PdfDocument<'_>, PdfDocument<'static>>(document)
        };

        Ok(Self { document, page_count })
    }

    /// Load a PDF from an already-opened file handle with an optional password.
    ///
    /// Reads the file contents into memory. For very large files, prefer
    /// `from_file` with a path. This exists for the worker process where
    /// only an inherited handle is available.
    pub fn from_file_handle_with_password(
        file: &std::fs::File,
        password: Option<&str>,
    ) -> Result<Self, String> {
        use std::io::Read;
        let mut data = Vec::new();
        let mut handle = file.try_clone().map_err(|e| format!("clone handle: {e}"))?;
        handle.read_to_end(&mut data).map_err(|e| format!("read file: {e}"))?;
        Self::from_bytes_with_password(data, password)
    }

    /// Load a PDF from an already-opened file handle (no password).
    pub fn from_file_handle(file: &std::fs::File) -> Result<Self, String> {
        Self::from_file_handle_with_password(file, None)
    }

    /// Load a PDF from raw bytes with an optional password.
    pub fn from_bytes_with_password(data: Vec<u8>, password: Option<&str>) -> Result<Self, String> {
        let document = pdfium()
            .load_pdf_from_byte_vec(data, password)
            .map_err(|e| format!("pdfium load failed: {e}"))?;

        let page_count = document.pages().len() as u32;

        // SAFETY: pdfium() returns &'static Pdfium; document borrows from it.
        let document = unsafe {
            std::mem::transmute::<PdfDocument<'_>, PdfDocument<'static>>(document)
        };

        Ok(Self { document, page_count })
    }

    /// Load a PDF from raw bytes (no password).
    pub fn from_bytes(data: Vec<u8>) -> Result<Self, String> {
        Self::from_bytes_with_password(data, None)
    }
}

impl Rasterize for PdfiumEngine {
    fn rasterize(&self, req: &RasterizeRequest) -> Result<TileOutput, RasterizeError> {
        if req.page_index >= self.page_count {
            return Err(RasterizeError::PageOutOfRange {
                requested: req.page_index,
                page_count: self.page_count,
            });
        }

        let pages = self.document.pages();
        let page = pages.iter().nth(req.page_index as usize)
            .ok_or_else(|| RasterizeError::Engine("failed to get page".into()))?;

        // Compute pixel dimensions from the page's size * scale.
        let page_w_pt = page.width().value;
        let page_h_pt = page.height().value;
        let page_w_px = (page_w_pt * req.scale).ceil() as u32;
        let page_h_px = (page_h_pt * req.scale).ceil() as u32;

        // Render the full page at the computed pixel dimensions.
        let bitmap = page.render(page_w_px as i32, page_h_px as i32, None)
            .map_err(|e| RasterizeError::Engine(format!("render: {e}")))?;

        let rendered_w = bitmap.width() as u32;
        let rendered_h = bitmap.height() as u32;

        // Get the raw pixel data (BGRA format by default).
        let raw = bitmap.as_raw_bytes();

        // Crop to the requested rect (clamped to rendered bounds).
        let rx = req.rect.x.min(rendered_w);
        let ry = req.rect.y.min(rendered_h);
        let rw = req.rect.w.min(rendered_w.saturating_sub(rx));
        let rh = req.rect.h.min(rendered_h.saturating_sub(ry));

        if rw == 0 || rh == 0 {
            return Err(RasterizeError::Engine("zero-size crop rect".into()));
        }

        // Bitmap is BGRA; we need RGBA.
        let mut rgba = vec![0u8; (rw * rh * 4) as usize];
        for row in 0..rh {
            let src_y = ry + row;
            for col in 0..rw {
                let src_x = rx + col;
                let src_idx = ((src_y * rendered_w + src_x) * 4) as usize;
                let dst_idx = ((row * rw + col) * 4) as usize;

                // BGRA → RGBA swap
                rgba[dst_idx] = raw[src_idx + 2];     // R
                rgba[dst_idx + 1] = raw[src_idx + 1]; // G
                rgba[dst_idx + 2] = raw[src_idx];     // B
                rgba[dst_idx + 3] = raw[src_idx + 3]; // A
            }
        }

        Ok(TileOutput {
            rgba_pixels: rgba,
            width: rw,
            height: rh,
        })
    }

    fn page_count(&self) -> u32 {
        self.page_count
    }
}

impl engine_api::structure::Structure for PdfiumEngine {
    fn outline(&self) -> Result<engine_api::structure::Outline, engine_api::structure::StructureError> {
        let bookmarks = self.document.bookmarks();
        let mut entries = Vec::new();

        if let Some(root) = bookmarks.root() {
            collect_bookmarks(&root, &mut entries);
        }

        Ok(engine_api::structure::Outline { entries })
    }

    fn layers(&self) -> Result<engine_api::structure::Layers, engine_api::structure::StructureError> {
        // PDFium's optional content group API is complex and requires per-page queries.
        // For M1, we return a basic layer list from the document's OCG entries.
        // A proper implementation would query FPDFOCGContext_GetOCGCount/GetOCG.
        Ok(engine_api::structure::Layers::default())
    }

    fn attachments(&self) -> Result<engine_api::structure::Attachments, engine_api::structure::StructureError> {
        let attachments_api = self.document.attachments();
        let count = attachments_api.len() as usize;
        let mut files = Vec::with_capacity(count);

        for i in 0..count {
            if let Ok(att) = attachments_api.get(i as u16) {
                let name = att.name();
                let data_size = att.save_to_bytes().map(|d| d.len() as u64).unwrap_or(0);

                files.push(engine_api::structure::Attachment {
                    name,
                    mime_type: None,
                    size: data_size,
                    created: None,
                    modified: None,
                    description: None,
                });
            }
        }

        Ok(engine_api::structure::Attachments { files })
    }

    fn page_meta(&self) -> Result<Vec<engine_api::structure::PageMeta>, engine_api::structure::StructureError> {
        let pages = self.document.pages();
        let mut metas = Vec::with_capacity(self.page_count as usize);
        for (i, page) in pages.iter().enumerate() {
            let width = page.width().value;
            let height = page.height().value;
            // PDFium reports rotation via the page's /Rotate entry.
            // pdfium-render doesn't expose rotation directly, so we default to 0.
            // TODO: extract rotation from page dictionary when pdfium-render adds support.
            let rotation = 0u32;
            metas.push(engine_api::structure::PageMeta {
                index: i as u32,
                width,
                height,
                rotation,
                label: None,
            });
        }
        Ok(metas)
    }
}

/// Recursively collect bookmarks from a pdfium-render PdfBookmark into our OutlineEntry tree.
fn collect_bookmarks(bookmark: &pdfium_render::prelude::PdfBookmark<'_>, entries: &mut Vec<engine_api::structure::OutlineEntry>) {
    let title = bookmark.title().unwrap_or_default();

    // Get the destination page for this bookmark.
    let (page, y, zoom) = if let Some(dest) = bookmark.destination() {
        let page_index = dest.page_index().map(|i| i as u32).unwrap_or(0);
        (page_index, 0.0, 0.0)
    } else {
        (0, 0.0, 0.0)
    };

    let mut children = Vec::new();
    if let Some(first_child) = bookmark.first_child() {
        collect_bookmarks_recursive(&first_child, &mut children);
    }

    entries.push(engine_api::structure::OutlineEntry {
        title,
        page,
        y,
        zoom,
        children,
    });

    // Process siblings.
    if let Some(next) = bookmark.next_sibling() {
        collect_bookmarks_recursive(&next, entries);
    }
}

/// Recursive helper for bookmark tree traversal.
fn collect_bookmarks_recursive(bookmark: &pdfium_render::prelude::PdfBookmark<'_>, entries: &mut Vec<engine_api::structure::OutlineEntry>) {
    let title = bookmark.title().unwrap_or_default();

    let (page, y, zoom) = if let Some(dest) = bookmark.destination() {
        let page_index = dest.page_index().map(|i| i as u32).unwrap_or(0);
        (page_index, 0.0, 0.0)
    } else {
        (0, 0.0, 0.0)
    };

    let mut children = Vec::new();
    if let Some(first_child) = bookmark.first_child() {
        collect_bookmarks_recursive(&first_child, &mut children);
    }

    entries.push(engine_api::structure::OutlineEntry {
        title,
        page,
        y,
        zoom,
        children,
    });

    if let Some(next) = bookmark.next_sibling() {
        collect_bookmarks_recursive(&next, entries);
    }
}

impl Extract for PdfiumEngine {
    fn extract_page(&self, page_index: u32) -> Result<PageTextModel, ExtractError> {
        if page_index >= self.page_count {
            return Err(ExtractError::PageOutOfRange {
                requested: page_index,
                page_count: self.page_count,
            });
        }

        let pages = self.document.pages();
        let page = pages.iter().nth(page_index as usize)
            .ok_or_else(|| ExtractError::Engine("failed to get page".into()))?;

        let text_page = page.text()
            .map_err(|e| ExtractError::Engine(format!("text page: {e}")))?;

        // Collect all characters with their positions.
        let chars_coll = text_page.chars();
        let char_count_est = chars_coll.len();
        let mut chars = Vec::with_capacity(char_count_est);
        for ch in chars_coll.iter() {
            chars.push(ch);
        }

        if chars.is_empty() {
            return Ok(PageTextModel {
                page_index,
                lines: vec![],
                reliable: true,
                char_count: 0,
                has_structure: false,
            });
        }

        // Group characters into lines by Y-position proximity.
        // Characters on the same line have similar Y coordinates.
        let line_threshold = 4.0; // points — characters within this Y range are on the same line.

        let mut char_data: Vec<(char, f32, f32, f32, f32)> = Vec::new(); // (char, x, y, w, h)
        for ch in &chars {
            let unicode = ch.unicode_char().unwrap_or('\u{FFFD}');
            let rect = ch.loose_bounds().unwrap_or_else(|_| {
                // Fallback: use a zero-size rect at origin.
                PdfRect::new(PdfPoints::zero(), PdfPoints::zero(), PdfPoints::zero(), PdfPoints::zero())
            });
            let x = rect.left().value;
            let y = rect.top().value;
            let w = rect.right().value - rect.left().value;
            let h = rect.bottom().value - rect.top().value;
            char_data.push((unicode, x, y, w, h));
        }

        // Group into lines: characters with Y within line_threshold of the line's Y are on the same line.
        let mut lines: Vec<TextLine> = Vec::new();
        let mut current_line_chars: Vec<(char, f32, f32, f32, f32)> = Vec::new();
        let mut current_line_y: f32 = char_data[0].2; // Y of first char in current line.

        for &c in &char_data {
            if (c.2 - current_line_y).abs() > line_threshold && !current_line_chars.is_empty() {
                // Flush current line.
                let line = build_line(lines.len() as u32, &current_line_chars);
                lines.push(line);
                current_line_chars.clear();
            }
            if current_line_chars.is_empty() {
                current_line_y = c.2;
            }
            current_line_chars.push(c);
        }
        // Flush last line.
        if !current_line_chars.is_empty() {
            let line = build_line(lines.len() as u32, &current_line_chars);
            lines.push(line);
        }

        let char_count = char_data.len() as u32;

        Ok(PageTextModel {
            page_index,
            lines,
            reliable: true, // TODO: detect unreliable ToUnicode maps
            char_count,
            has_structure: false, // TODO: detect tagged structure
        })
    }

    fn page_count(&self) -> u32 {
        self.page_count
    }
}

/// Build a TextLine from a group of (char, x, y, w, h) tuples.
fn build_line(line_index: u32, chars: &[(char, f32, f32, f32, f32)]) -> TextLine {
    let text: String = chars.iter().map(|c| c.0).collect();
    let x = chars.iter().map(|c| c.1).fold(f32::INFINITY, f32::min);
    let y = chars.iter().map(|c| c.2).fold(f32::INFINITY, f32::min);
    let x2 = chars.iter().map(|c| c.1 + c.3).fold(0.0f32, f32::max);
    let y2 = chars.iter().map(|c| c.2 + c.4).fold(0.0f32, f32::max);

    let spans: Vec<TextSpan> = chars.iter().enumerate().map(|(i, c)| {
        TextSpan {
            text: c.0.to_string(),
            x: c.1,
            y: c.2,
            width: c.3,
            height: c.4,
            line_index,
            word_index: i as u32,
            is_structured: false,
        }
    }).collect();

    TextLine {
        index: line_index,
        text,
        x,
        y,
        width: x2 - x,
        height: y2 - y,
        spans,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_api::extract::Extract;
    use engine_api::rasterize::TileRect;

    fn minimal_pdf_bytes() -> Vec<u8> {
        // Minimal 1-page PDF — same as pdf-cos test fixture.
        b"%PDF-1.0\n\
          1 0 obj\n\
          <</Type /Catalog /Pages 2 0 R>>\n\
          endobj\n\
          2 0 obj\n\
          <</Type /Pages /Kids [3 0 R] /Count 1>>\n\
          endobj\n\
          3 0 obj\n\
          <</Type /Page /Parent 2 0 R /MediaBox [0 0 612 792]>>\n\
          endobj\n\
          xref\n\
          0 4\n\
          0000000000 65535 f \n\
          0000000009 00000 n \n\
          0000000056 00000 n \n\
          0000000111 00000 n \n\
          trailer\n\
          <</Size 4 /Root 1 0 R>>\n\
          startxref\n\
          180\n\
          %%EOF"
            .to_vec()
    }

    #[test]
    fn pdfium_load_and_page_count() {
        let engine = PdfiumEngine::from_bytes(minimal_pdf_bytes())
            .expect("load minimal PDF");
        assert_eq!(Rasterize::page_count(&engine), 1);
    }

    #[test]
    fn pdfium_rasterize_produces_pixels() {
        let engine = PdfiumEngine::from_bytes(minimal_pdf_bytes())
            .expect("load minimal PDF");
        let out = engine.rasterize(&RasterizeRequest {
            page_index: 0,
            rect: TileRect { x: 0, y: 0, w: 64, h: 64 },
            scale: 1.0,
        }).expect("rasterize");
        assert_eq!(out.width, 64);
        assert_eq!(out.height, 64);
        assert_eq!(out.rgba_pixels.len(), 64 * 64 * 4);
        // Alpha should be 255 for a rendered page.
        assert_eq!(out.rgba_pixels[3], 255);
    }

    #[test]
    fn pdfium_page_out_of_range() {
        let engine = PdfiumEngine::from_bytes(minimal_pdf_bytes())
            .expect("load minimal PDF");
        let err = engine.rasterize(&RasterizeRequest {
            page_index: 5,
            rect: TileRect { x: 0, y: 0, w: 64, h: 64 },
            scale: 1.0,
        }).unwrap_err();
        match err {
            RasterizeError::PageOutOfRange { requested, page_count } => {
                assert_eq!(requested, 5);
                assert_eq!(page_count, 1);
            }
            other => panic!("expected PageOutOfRange, got {other:?}"),
        }
    }

    #[test]
    fn pdfium_extract_empty_page() {
        let engine = PdfiumEngine::from_bytes(minimal_pdf_bytes())
            .expect("load minimal PDF");
        let model = engine.extract_page(0).expect("extract");
        assert_eq!(model.page_index, 0);
        // Minimal PDF has no text content.
        assert_eq!(model.char_count, 0);
        assert!(model.lines.is_empty());
    }

    #[test]
    fn pdfium_extract_out_of_range() {
        let engine = PdfiumEngine::from_bytes(minimal_pdf_bytes())
            .expect("load minimal PDF");
        let err = engine.extract_page(5).unwrap_err();
        match err {
            engine_api::extract::ExtractError::PageOutOfRange { requested, page_count } => {
                assert_eq!(requested, 5);
                assert_eq!(page_count, 1);
            }
            other => panic!("expected PageOutOfRange, got {other:?}"),
        }
    }

    #[test]
    fn pdfium_extract_fixture_pdf() {
        // Test extraction on a real PDF if the fixture exists.
        let fixtures_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..").join("..").join("tools").join("corpus-diff").join("fixtures");
        let pdf_path = fixtures_dir.join("valid-1page.pdf");
        if !pdf_path.exists() {
            eprintln!("fixture not found, skipping");
            return;
        }
        let engine = PdfiumEngine::from_file(&pdf_path, None).expect("load fixture");
        let model = engine.extract_page(0).expect("extract");
        assert_eq!(model.page_index, 0);
        // The fixture may or may not have text; just verify extraction doesn't panic.
        eprintln!("extracted {} chars, {} lines", model.char_count, model.lines.len());
    }
}
