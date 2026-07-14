//! PDFium backend impl of engine-api traits. [ADR-005, GR-4]
//!
//! Uses `pdfium-render` for safe bindings + prebuilt PDFium binaries.
//! SAFETY: all unsafe lives here; every unsafe block must carry a // SAFETY: comment. [ADR-027]

use std::sync::Once;

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

#[cfg(test)]
mod tests {
    use super::*;
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
        assert_eq!(engine.page_count(), 1);
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
}
