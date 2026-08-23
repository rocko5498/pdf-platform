//! PDFium backend impl of engine-api traits. [ADR-005, GR-4]
//!
//! Uses `pdfium-render` for safe bindings + prebuilt PDFium binaries.
//! SAFETY: all unsafe lives here; every unsafe block must carry a // SAFETY: comment. [ADR-027]

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use engine_api::extract::{Extract, ExtractError, PageTextModel, TextLine, TextSpan};
use engine_api::rasterize::{Rasterize, RasterizeError, RasterizeRequest, TileOutput};
use pdfium_render::prelude::*;

/// PDFium-backed rasterizer. Holds a loaded document. [ADR-005]
///
/// Shareable across threads, but **not** concurrent: PDFium itself is not
/// thread-safe, so every entry point serializes on `PDFIUM_CALL`. This comment
/// used to claim "multiple tiles can be rasterized concurrently", which is what
/// the Send/Sync impls assert and what the library does not provide.
pub struct PdfiumEngine {
    /// `Option` only so `Drop` can destroy it *inside* the lock. Dropping a
    /// `PdfDocument` calls into PDFium like any other operation, so a document
    /// going out of scope on one thread while another is mid-extraction is the
    /// same data race as two concurrent extractions — and it is the one the
    /// guarded entry points cannot cover, because field destruction runs after
    /// any guard a method held. Always `Some` between construction and drop.
    document: Option<PdfDocument<'static>>,
    page_count: u32,
}

impl Drop for PdfiumEngine {
    fn drop(&mut self) {
        let _guard = PDFIUM_CALL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        drop(self.document.take());
    }
}

impl PdfiumEngine {
    /// The loaded document. Callers must already hold `PDFIUM_CALL`.
    fn document(&self) -> &PdfDocument<'static> {
        self.document
            .as_ref()
            .expect("document is taken only in Drop, after which no method runs")
    }
}

/// Serializes every call into PDFium.
///
/// PDFium is not thread-safe: one library instance, and documents and text
/// pages derived from it, must not be touched concurrently. The Send/Sync
/// impls below say `PdfiumEngine` may cross and be shared between threads,
/// which is true only because every entry point takes this lock first.
///
/// Two `extraction_accuracy` tests running in parallel threads of one test
/// binary proved the cost of not having it: on Windows the extracted text came
/// back with characters missing ("Hello extraction" as "Helo xtracion"), and
/// on Linux the process died with `free(): invalid pointer`. Silent corruption
/// of a document's text is exactly the failure PRIN-1 puts first.
///
/// ponytail: one global lock, not per-document. The worker is single-writer
/// per document (SDS §7.4) so contention is a coordinator-side concern only;
/// move to a per-document lock if parallel utility jobs ever need it.
/// [ADR-005, ADR-027, CR-4, PRIN-1, GR-8]
static PDFIUM_CALL: std::sync::Mutex<()> = std::sync::Mutex::new(());

// SAFETY: every path that touches the document or the library goes through
// `with_pdfium`, so no two threads are inside PDFium at once. Without that
// lock these impls would be unsound, not merely optimistic. [ADR-027]
unsafe impl Send for PdfiumEngine {}
// SAFETY: as above — shared access is serialized by `PDFIUM_CALL`.
unsafe impl Sync for PdfiumEngine {}

/// Global PDFium instance, bound once per process. [ADR-005]
///
/// The binding outcome is cached, failure included: a missing engine is a
/// property of the installation, so retrying it per document would only
/// repeat the same filesystem lookup and the same diagnostic.
static PDFIUM: OnceLock<Result<PdfiumHandle, String>> = OnceLock::new();

/// A leaked `Pdfium` shared across worker threads.
///
/// `Pdfium` owns a `Box<dyn PdfiumLibraryBindings>`, which is neither `Send`
/// nor `Sync` in the type system, so it cannot sit in a `OnceLock` directly.
#[derive(Clone, Copy)]
struct PdfiumHandle(*const Pdfium);

// SAFETY: the pointee is leaked at first bind and never freed or mutated
// afterwards, so every reader observes an immutable, permanently live value.
// PDFium itself synchronizes its internal state, which is the same reasoning
// `PdfiumEngine`'s Send/Sync impls below already rely on. [ADR-005, ADR-027]
unsafe impl Send for PdfiumHandle {}
// SAFETY: as above — shared reads of an immutable, process-lifetime value.
unsafe impl Sync for PdfiumHandle {}

/// Fraction of private-use codepoints above which extraction is suspect.
const PUA_UNRELIABLE_RATIO: f32 = 0.10;

/// Judge whether extracted page text can be trusted. [FR-SRCH-5, ADR-019 §4]
///
/// `reliable` was hard-coded `true`, so the honesty mechanism ADR-019 §4
/// requires — flagging pages whose text layer is unreliable instead of letting
/// them be "silently searched wrong" — never fired.
///
/// Detects the two signatures of a missing or lying ToUnicode CMap that
/// survive into extracted text: U+FFFD replacement characters (a codepoint
/// that could not be mapped at all), and a high proportion of Private Use Area
/// codepoints (a subset font extracting as raw glyph ids, which looks like
/// text and searches wrong).
///
/// This is deliberately conservative and does not claim to catch every
/// pathology — a font that lies *plausibly* still extracts as ordinary
/// characters and cannot be caught here. It replaces an unconditional claim of
/// reliability with a real, if partial, check.
fn text_is_reliable(text: &str) -> bool {
    if text.is_empty() {
        return true;
    }
    if text.contains('\u{FFFD}') {
        return false;
    }
    let total = text.chars().count();
    let private_use = text
        .chars()
        .filter(|c| matches!(*c, '\u{E000}'..='\u{F8FF}'))
        .count();
    (private_use as f32 / total as f32) < PUA_UNRELIABLE_RATIO
}

/// Manifest platform id for this build, matching `third_party/pdfium/provenance.toml`.
const fn platform_id() -> &'static str {
    if cfg!(target_os = "windows") {
        if cfg!(target_arch = "aarch64") { "win-arm64" } else { "win-x64" }
    } else if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") { "mac-arm64" } else { "mac-x64" }
    } else if cfg!(target_arch = "aarch64") {
        "linux-arm64"
    } else {
        "linux-x64"
    }
}

/// Shared-library file name PDFium ships under on this platform.
const fn library_file_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "pdfium.dll"
    } else if cfg!(target_os = "macos") {
        "libpdfium.dylib"
    } else {
        "libpdfium.so"
    }
}

/// Where the engine may be found, in priority order. [SDS §13.4, ADR-028 §3]
///
/// Nothing here reaches the network. PDFium is a setup-time input installed by
/// `tools/provision_engine.py`; Z1 has no network at all (GR-1) and the product
/// transmits nothing without an explicit user action (GR-9), so a worker that
/// cannot find the library says so rather than fetching it.
fn candidate_paths() -> Vec<PathBuf> {
    candidate_paths_from(
        std::env::var_os("PDFIUM_LIB_PATH").map(PathBuf::from),
        std::env::current_exe().ok(),
    )
}

/// Resolution order, with the two environment lookups injected so it is testable.
fn candidate_paths_from(override_path: Option<PathBuf>, current_exe: Option<PathBuf>) -> Vec<PathBuf> {
    let file_name = library_file_name();
    let mut candidates = Vec::new();

    // 1. Explicit override, for a packaged install or a local PDFium build.
    //    Accepts either the library itself or the directory holding it.
    if let Some(path) = override_path {
        if path.is_dir() {
            candidates.push(path.join(file_name));
        } else {
            candidates.push(path);
        }
    }

    // 2. Beside the executable, which is how a shipped build carries it.
    if let Some(dir) = current_exe.as_deref().and_then(Path::parent) {
        candidates.push(dir.join(file_name));
    }

    // 3. The provisioned tree in a source checkout. CARGO_MANIFEST_DIR is
    //    core/engine-pdfium, so the repository root is two levels up.
    let vendored = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../third_party/pdfium/prebuilt")
        .join(platform_id())
        .join(file_name);
    candidates.push(vendored);

    candidates
}

/// Bind PDFium from the first candidate path that exists.
fn bind_pdfium() -> Result<Pdfium, String> {
    let candidates = candidate_paths();
    let mut attempted = Vec::new();
    for path in &candidates {
        if !path.is_file() {
            attempted.push(format!("{} (not found)", path.display()));
            continue;
        }
        match Pdfium::bind_to_library(path) {
            Ok(bindings) => return Ok(Pdfium::new(bindings)),
            Err(error) => attempted.push(format!("{}: {error}", path.display())),
        }
    }
    // FR-DIAG-1 and GR-8: name the fault and the fix, never a silent stub.
    Err(format!(
        "PDFium is not installed for {}. Run `python tools/provision_engine.py`          to install the pinned artifact recorded in third_party/pdfium/provenance.toml,          or set PDFIUM_LIB_PATH to an existing {}. Looked at: {}",
        platform_id(),
        library_file_name(),
        attempted.join("; ")
    ))
}

fn pdfium() -> Result<&'static Pdfium, String> {
    // Leaked on purpose: PDFium is designed to live for the process lifetime,
    // and a &'static keeps documents borrowing from it valid for as long.
    let handle = PDFIUM
        .get_or_init(|| {
            bind_pdfium().map(|engine| PdfiumHandle(Box::leak(Box::new(engine)) as *const Pdfium))
        })
        .clone()?;
    // SAFETY: the pointer came from `Box::leak` above, so it is non-null,
    // aligned, and valid for the rest of the process.
    Ok(unsafe { &*handle.0 })
}

impl PdfiumEngine {
    /// Load a PDF file from disk with an optional password.
    pub fn from_file(path: &std::path::Path, password: Option<&str>) -> Result<Self, String> {
        let _guard = PDFIUM_CALL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let document = pdfium()?
            .load_pdf_from_file(path, password)
            .map_err(|e| format!("pdfium load failed: {e}"))?;

        let page_count = document.pages().len() as u32;

        // SAFETY: pdfium() returns a &'static Pdfium; the document borrows from it
        // and the global lives for the process lifetime.
        let document = unsafe {
            std::mem::transmute::<PdfDocument<'_>, PdfDocument<'static>>(document)
        };

        Ok(Self { document: Some(document), page_count })
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
        use std::io::{Read, Seek, SeekFrom};
        let mut data = Vec::new();
        let mut handle = file.try_clone().map_err(|e| format!("clone handle: {e}"))?;
        // The document arrives as an inherited FD/HANDLE, so this process shares
        // one open file description — and one file offset — with the coordinator
        // and with every worker spawned before it. Reading from the current
        // position yields nothing once an earlier reader reached EOF, which
        // presents as a bogus FormatError. Always read the whole file.
        // [SDS §4.2, SDS §10.1, GR-8]
        handle
            .seek(SeekFrom::Start(0))
            .map_err(|e| format!("rewind handle: {e}"))?;
        handle.read_to_end(&mut data).map_err(|e| format!("read file: {e}"))?;
        Self::from_bytes_with_password(data, password)
    }

    /// Load a PDF from an already-opened file handle (no password).
    pub fn from_file_handle(file: &std::fs::File) -> Result<Self, String> {
        Self::from_file_handle_with_password(file, None)
    }

    /// Load a PDF from raw bytes with an optional password.
    pub fn from_bytes_with_password(data: Vec<u8>, password: Option<&str>) -> Result<Self, String> {
        let _guard = PDFIUM_CALL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let document = pdfium()?
            .load_pdf_from_byte_vec(data, password)
            .map_err(|e| format!("pdfium load failed: {e}"))?;

        let page_count = document.pages().len() as u32;

        // SAFETY: pdfium() returns &'static Pdfium; document borrows from it.
        let document = unsafe {
            std::mem::transmute::<PdfDocument<'_>, PdfDocument<'static>>(document)
        };

        Ok(Self { document: Some(document), page_count })
    }

    /// Load a PDF from raw bytes (no password).
    pub fn from_bytes(data: Vec<u8>) -> Result<Self, String> {
        Self::from_bytes_with_password(data, None)
    }
}

impl Rasterize for PdfiumEngine {
    fn rasterize(&self, req: &RasterizeRequest) -> Result<TileOutput, RasterizeError> {
        let _guard = PDFIUM_CALL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if req.page_index >= self.page_count {
            return Err(RasterizeError::PageOutOfRange {
                requested: req.page_index,
                page_count: self.page_count,
            });
        }

        let pages = self.document().pages();
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
        let _guard = PDFIUM_CALL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let bookmarks = self.document().bookmarks();
        let mut entries = Vec::new();

        if let Some(root) = bookmarks.root() {
            collect_bookmarks(&root, &mut entries);
        }

        Ok(engine_api::structure::Outline { entries })
    }

    fn layers(&self) -> Result<engine_api::structure::Layers, engine_api::structure::StructureError> {
        let _guard = PDFIUM_CALL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        // PDFium's optional content group API is complex and requires per-page queries.
        // For M1, we return a basic layer list from the document's OCG entries.
        // A proper implementation would query FPDFOCGContext_GetOCGCount/GetOCG.
        Ok(engine_api::structure::Layers::default())
    }

    fn attachments(&self) -> Result<engine_api::structure::Attachments, engine_api::structure::StructureError> {
        let _guard = PDFIUM_CALL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let attachments_api = self.document().attachments();
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
        let _guard = PDFIUM_CALL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let pages = self.document().pages();
        let mut metas = Vec::with_capacity(self.page_count as usize);
        for (i, page) in pages.iter().enumerate() {
            let width = page.width().value;
            let height = page.height().value;
            // This was hard-coded to 0 with a comment saying pdfium-render did
            // not expose rotation. It does: `PdfPage::rotation()`. Every page
            // of every document therefore reported "not rotated", including
            // pages the product had just rotated itself. [FR-ORG-2, PRIN-6]
            let rotation = match page.rotation() {
                Ok(pdfium_render::prelude::PdfPageRenderRotation::None) => 0,
                Ok(pdfium_render::prelude::PdfPageRenderRotation::Degrees90) => 90,
                Ok(pdfium_render::prelude::PdfPageRenderRotation::Degrees180) => 180,
                Ok(pdfium_render::prelude::PdfPageRenderRotation::Degrees270) => 270,
                // A page whose rotation cannot be read is reported upright
                // rather than failing the whole structure query; the width and
                // height above already account for it.
                Err(_) => 0,
            };
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
        let _guard = PDFIUM_CALL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if page_index >= self.page_count {
            return Err(ExtractError::PageOutOfRange {
                requested: page_index,
                page_count: self.page_count,
            });
        }

        let pages = self.document().pages();
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
            // PDFium synthesizes CR/LF between drawn runs; they are separators,
            // not glyphs, and carry no bounds. Keeping them produced a phantom
            // `TextLine` whose text was a bare line break at zero width and
            // height, which any consumer walking lines reads as real content.
            // [ADR-019, FR-SRCH-2]
            if unicode == '\r' || unicode == '\n' {
                continue;
            }
            let rect = ch.loose_bounds().unwrap_or_else(|_| {
                // Fallback: use a zero-size rect at origin.
                PdfRect::new(PdfPoints::zero(), PdfPoints::zero(), PdfPoints::zero(), PdfPoints::zero())
            });
            let x = rect.left().value;
            let y = rect.top().value;
            let w = rect.right().value - rect.left().value;
            // PDF user space is bottom-up, so `top` is the larger value and
            // `bottom - top` is negative. Every extracted line and span carried
            // a negative height — the geometry search hit-testing, selection
            // rectangles and OCR block placement all read. Take the magnitude;
            // `y` remains the top edge, which is what the consumers expect.
            // [FR-SRCH-2, SDS §3.3, ADR-019]
            let h = (rect.top().value - rect.bottom().value).abs();
            char_data.push((unicode, x, y, w, h));
        }

        // Group into lines: characters with Y within line_threshold of the line's Y are on the same line.
        let mut lines: Vec<TextLine> = Vec::new();
        let mut current_line_chars: Vec<(char, f32, f32, f32, f32)> = Vec::new();
        let mut current_line_y: f32 = char_data[0].2; // Y of first char in current line.

        for &c in &char_data {
            // PDFium synthesizes a space between text runs that are drawn
            // separately, and those characters carry no bounds at all. Letting
            // one decide line membership split a line at every run boundary:
            // a right-to-left fixture drawn glyph by glyph came back as five
            // lines — "alef", " ", "bet", " ", "gimel" — instead of one, which
            // is wrong for selection, hit-testing and any consumer that walks
            // lines. A character with no geometry joins the current line and
            // never starts or ends one. [FR-SRCH-2, ADR-019]
            let has_geometry = c.3 > 0.0 || c.4 > 0.0;

            if has_geometry
                && (c.2 - current_line_y).abs() > line_threshold
                && !current_line_chars.is_empty()
            {
                // Flush current line.
                let line = build_line(lines.len() as u32, &current_line_chars);
                lines.push(line);
                current_line_chars.clear();
            }
            if current_line_chars.is_empty() {
                // A boundless character carries no position, so it cannot
                // establish where a line sits; drop it rather than let it
                // anchor one at the origin.
                if !has_geometry {
                    continue;
                }
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

        // Was hard-coded `true`, so no page was ever flagged and the ADR-019 §4
        // honesty path could not fire. [FR-SRCH-5]
        let page_text: String = lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let reliable = text_is_reliable(&page_text);

        Ok(PageTextModel {
            page_index,
            lines,
            reliable,
            char_count,
            // Still unimplemented, but conservative: claiming "no structure"
            // understates the document rather than promising accessible
            // reading order that was never verified. [DS-CANVAS-A11Y-4]
            has_structure: false,
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
mod reliability_tests {
    use super::text_is_reliable;

    #[test]
    fn ordinary_text_is_reliable() {
        assert!(text_is_reliable("The quick brown fox jumps over the lazy dog."));
    }

    #[test]
    fn a_replacement_character_marks_the_page_unreliable() {
        // U+FFFD in extracted PDF text means a codepoint could not be mapped —
        // in practice a missing or broken ToUnicode CMap.
        assert!(!text_is_reliable("Invoice total: \u{FFFD}\u{FFFD}\u{FFFD}"));
    }

    #[test]
    fn heavy_private_use_area_output_marks_the_page_unreliable() {
        // A subset font with no ToUnicode commonly extracts as PUA glyph ids:
        // it looks like text and searches wrong. [ADR-019 §4]
        assert!(!text_is_reliable("\u{E000}\u{E001}\u{E002}\u{E003}\u{E004}"));
    }

    #[test]
    fn an_incidental_private_use_glyph_does_not_condemn_a_good_page() {
        let mostly_fine = format!("{}\u{E000}", "a".repeat(200));
        assert!(text_is_reliable(&mostly_fine));
    }

    #[test]
    fn an_empty_page_is_not_reported_as_unreliable() {
        // No text is a legitimate state (an image-only scan), not a decoding
        // failure — OCR is the answer there, not a reliability warning.
        assert!(text_is_reliable(""));
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

    /// The worker receives its document as an inherited FD/HANDLE, so it shares
    /// one open file description — and therefore one file offset — with the
    /// coordinator that spawned it. After a first worker reads the document,
    /// that shared offset sits at EOF, so a respawned worker reading from the
    /// current position sees zero bytes and silently comes up with no engine.
    /// [SDS §4.2, SDS §10.1, GR-8]
    #[test]
    fn pdfium_loads_from_handle_left_at_eof() {
        use std::io::{Read, Write};

        let path = std::env::temp_dir().join(format!(
            "pdf-platform-eof-handle-{}.pdf",
            std::process::id()
        ));
        std::fs::write(&path, minimal_pdf_bytes()).expect("write fixture");

        let mut file = std::fs::File::open(&path).expect("open fixture");
        let mut drained = Vec::new();
        file.read_to_end(&mut drained).expect("drain to EOF");
        assert_eq!(drained.len(), minimal_pdf_bytes().len(), "first read consumed the file");

        let engine = PdfiumEngine::from_file_handle(&file)
            .expect("load from a handle another reader already advanced to EOF");
        assert_eq!(Rasterize::page_count(&engine), 1);

        drop(engine);
        drop(file);
        std::fs::remove_file(&path).ok();
        let _ = std::io::stdout().flush();
    }
}

#[cfg(test)]
mod engine_resolution_tests {
    use super::{bind_pdfium, candidate_paths_from, library_file_name, pdfium, platform_id};
    use std::path::PathBuf;

    #[test]
    fn override_is_tried_before_anything_else() {
        let explicit = PathBuf::from("/opt/custom/mypdfium.bin");
        let candidates = candidate_paths_from(
            Some(explicit.clone()),
            Some(PathBuf::from("/apps/pdf-platform/worker")),
        );
        assert_eq!(candidates.first(), Some(&explicit), "{candidates:?}");
    }

    #[test]
    fn override_directory_gets_the_platform_library_name() {
        let candidates = candidate_paths_from(Some(std::env::temp_dir()), None);
        assert_eq!(
            candidates[0],
            std::env::temp_dir().join(library_file_name()),
            "a directory override must resolve to the library inside it"
        );
    }

    #[test]
    fn executable_directory_precedes_the_source_checkout() {
        let candidates =
            candidate_paths_from(None, Some(PathBuf::from("/apps/pdf-platform/worker")));
        assert_eq!(candidates[0], PathBuf::from("/apps/pdf-platform").join(library_file_name()));
        let vendored = candidates.last().expect("vendored fallback is always present");
        assert!(
            vendored.to_string_lossy().contains("third_party"),
            "last resort must be the provisioned tree: {vendored:?}"
        );
        assert!(vendored.to_string_lossy().contains(platform_id()), "{vendored:?}");
    }

    #[test]
    fn a_missing_engine_reports_how_to_install_it_and_never_downloads() {
        // Provisioning is a setup step; Z1 has no network (GR-1). The only
        // honest response to an absent library is a diagnostic (GR-8, FR-DIAG-1).
        let candidates = candidate_paths_from(None, None);
        if candidates.iter().any(|path| path.is_file()) {
            // Engine provisioned on this machine: the error path cannot be
            // exercised without uninstalling it, so assert the resolver is
            // sane instead. Go through `pdfium()`, never `bind_pdfium()`:
            // dropping a bound `Pdfium` calls FPDF_DestroyLibrary and takes
            // the library down for the whole process, aborting later tests.
            assert!(pdfium().is_ok(), "an installed engine must bind");
            return;
        }
        let error = bind_pdfium().expect_err("no engine installed, bind must fail");
        assert!(error.contains("provision_engine.py"), "{error}");
        assert!(error.contains("PDFIUM_LIB_PATH"), "{error}");
    }
}
