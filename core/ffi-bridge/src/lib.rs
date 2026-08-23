//! Sole RustÃƒÆ’Ã‚Â¢ÃƒÂ¢Ã¢â€šÂ¬Ã‚Â ÃƒÂ¢Ã¢â€šÂ¬Ã‚ÂQt FFI boundary. [ADR-004]
//!
//! RULES (enforced in review):
//!   FFI-1: cxx-checked interface only ÃƒÆ’Ã‚Â¢ÃƒÂ¢Ã¢â‚¬Å¡Ã‚Â¬ÃƒÂ¢Ã¢â€šÂ¬Ã‚Â no hand-rolled ABI.
//!   FFI-3: no raw pointers owned across the boundary.
//!   FFI-4: carries commands/events/handles only ÃƒÆ’Ã‚Â¢ÃƒÂ¢Ã¢â‚¬Å¡Ã‚Â¬ÃƒÂ¢Ã¢â€šÂ¬Ã‚Â never document objects.
//!   FFI-6: two-reviewer rule; changes require one FFI-surface owner. [ADR-027]
// SAFETY: cxx guarantees type-checked cross-language calls; no exceptions cross
//         this boundary; ownership does not straddle languages. [ADR-004, ADR-027]
#![allow(static_mut_refs)] // single-threaded Qt main thread; multi-doc later

// NOTE: `use std::os::windows::io::AsRawHandle;` was here, ungated and unused.
// `std::os::windows` does not exist on Linux or macOS, so it failed the build
// on two of the three platforms CI covers — `main` has been red since
// 2026-07-21 with `error[E0433]: cannot find 'windows' in 'os'`. The two other
// Windows-only imports in this workspace (sandbox/spawn.rs, sandbox/transport.rs)
// sit inside `#[cfg(windows)] mod windows` blocks, which is the correct
// pattern. Nothing in this file calls `as_raw_handle`, so the import is simply
// deleted rather than gated. [CMP-XPLAT-1, ADR-029]

use pdf_model::annotation::{
    Annotation, AnnotationStore, AnnotationType, Color, Rect, TextMarkupKind,
};
use pdf_model::appearance::{
    build_annotation_pdf_objects, build_widget_pdf_objects, generate_appearance,
};
use pdf_model::form::{AcroForm, FieldCalculation, FieldRect, FieldType, FieldValue, FormField};
use pdf_model::form_import::import_acroform_from_bytes;
use pdf_model::forms_js::run_form_calculations;
use pdf_model::overlay::CowOverlay;
use pdf_model::page_patch::inject_annot_refs;
use pdf_write::IncrementalWriter;
use pdf_model::fdf::{export_xfdf, import_xfdf_to_store};
use protocol::commands::{encode_command, Command};
use protocol::events::{decode_worker_event, WorkerEvent};
use protocol::handles::TILE_RGBA8_BYTES;
use protocol::inspect::StructuralSummary;
use sandbox::shmem::SharedRegion;
use sandbox::spawn::{spawn_worker_with_attachments, SpawnAttachments};
use search::{find_all, FindOptions};

/// Holds the state for one open document's worker connection + local model.
struct DocSession {
    child: sandbox::spawn::WorkerChild,
    #[allow(dead_code)]
    region: SharedRegion,
    next_cid: u64,
    page_count: u32,
    page_width: f32,
    page_height: f32,
    summary: StructuralSummary,
    /// Local annotation store for shell-authored markups. [FR-ANNOT, M4]
    annotations: AnnotationStore,
    /// Session AcroForm fill model (widget AP regen). [FR-FORM, FR-JS, M5]
    form: AcroForm,
    /// Honesty notes from the last form import.
    form_import_notes: Vec<String>,
    /// Form undo stack. [FR-FORM-6]
    form_undo_stack: Vec<FormUndoEntry>,
    // The undo/redo stacks below are deliberately unbounded: SDS §14 M3's
    // deliverable is "unlimited undo/redo", so capping them would trade a
    // stated product guarantee for memory. Everything else that grows with
    // document size or session length carries a bound. [GR-7, SDS §14 M3]
    /// Form redo stack. [FR-FORM-6]
    form_redo_stack: Vec<FormUndoEntry>,
    /// Annotation undo stack. [FR-ANNOT-4, M4]
    annot_undo_stack: Vec<AnnotUndoEntry>,
    /// Annotation redo stack. [FR-ANNOT-4, M4]
    annot_redo_stack: Vec<AnnotUndoEntry>,
    /// Cached page text for find/copy, bounded by total size. [GR-7]
    text_cache: TextCache,
    path: String,
}

/// How much extracted text one session keeps.
///
/// `find_text` extracts *every* page, so a long document put its entire text
/// into a `HashMap` that nothing ever evicted: the cache grew with document
/// size and session length, which is precisely what GR-7 forbids without a
/// declared bound. Eight mebibytes is roughly a 4,000-page text document; past
/// that, the least recently used page is dropped and re-extracted if it is
/// asked for again. [GR-7, ADR-011, SDS §9.4]
const TEXT_CACHE_MAX_BYTES: usize = 8 * 1024 * 1024;

/// A least-recently-used cache of extracted page text. [GR-7]
#[derive(Default)]
struct TextCache {
    pages: std::collections::HashMap<u32, CachedPageText>,
    /// Least recently used first.
    order: std::collections::VecDeque<u32>,
    bytes: usize,
}

impl TextCache {
    fn contains(&self, page: u32) -> bool {
        self.pages.contains_key(&page)
    }

    fn get(&mut self, page: u32) -> Option<&CachedPageText> {
        if self.pages.contains_key(&page) {
            self.touch(page);
        }
        self.pages.get(&page)
    }

    fn touch(&mut self, page: u32) {
        if let Some(at) = self.order.iter().position(|p| *p == page) {
            self.order.remove(at);
        }
        self.order.push_back(page);
    }

    fn insert(&mut self, page: u32, text: CachedPageText) {
        let size = text.size_bytes();
        if let Some(previous) = self.pages.remove(&page) {
            self.bytes = self.bytes.saturating_sub(previous.size_bytes());
        }
        self.pages.insert(page, text);
        self.bytes += size;
        self.touch(page);

        // Never evict the page just inserted: a caller that asked for it is
        // about to read it, and a page larger than the whole budget would
        // otherwise be evicted immediately and re-extracted forever.
        while self.bytes > TEXT_CACHE_MAX_BYTES && self.order.len() > 1 {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if let Some(dropped) = self.pages.remove(&oldest) {
                self.bytes = self.bytes.saturating_sub(dropped.size_bytes());
            }
        }
    }

    fn len(&self) -> usize {
        self.pages.len()
    }

}

struct CachedPageText {
    full_text: String,
    reliable: bool,
    line_geom: Vec<String>,
}

impl CachedPageText {
    /// Roughly what this entry holds, for the cache's budget. [GR-7]
    fn size_bytes(&self) -> usize {
        self.full_text.len()
            + self.line_geom.iter().map(String::len).sum::<usize>()
            + std::mem::size_of::<Self>()
    }
}

/// A single undoable form field change. [FR-FORM-6]
#[derive(Debug, Clone)]
struct FormUndoEntry {
    field_name: String,
    old_value: FieldValue,
    new_value: FieldValue,
}

/// A single undoable annotation operation. [FR-ANNOT-4, M4]
#[derive(Debug, Clone)]
enum AnnotUndoEntry {
    /// Annotation was created — undo removes it, redo puts it back.
    ///
    /// Carries the annotation itself, not just its id. Holding only an id made
    /// redo impossible: `redo_impl` had to return "redo not available for
    /// create" — as an `Ok`, so the shell reported success and nothing
    /// happened, while `can_redo()` kept saying yes. [FR-ANNOT-4, GR-8]
    Created { annotation: pdf_model::annotation::Annotation, page_index: u32 },
    /// Annotation was deleted -- undo re-adds it.
    Deleted { annotation: pdf_model::annotation::Annotation, page_index: u32 },
}

/// Global session storage (one document at a time until multi-doc).
// SAFETY: accessed only from the Qt main thread (single-threaded FFI).
#[allow(static_mut_refs)]
static mut SESSION: Option<Box<DocSession>> = None;

fn with_session_mut<T>(f: impl FnOnce(&mut DocSession) -> Result<T, String>) -> Result<T, String> {
    // SAFETY: single-threaded Qt main thread.
    let session = unsafe { SESSION.as_mut().ok_or("no open document")? };
    f(session)
}

fn send_recv(session: &mut DocSession, cmd: Command) -> Result<WorkerEvent, String> {
    let body = encode_command(&cmd);
    session
        .child
        .transport
        .send(&body)
        .map_err(|e| e.to_string())?;
    let reply = session
        .child
        .transport
        .recv_timeout(std::time::Duration::from_secs(30))
        .map_err(|e| e.to_string())?;
    decode_worker_event(&reply).map_err(|e| e.to_string())
}

fn next_cid(session: &mut DocSession) -> u64 {
    let id = session.next_cid;
    session.next_cid += 1;
    id
}

/// Open a document: spawn worker (optional password), create shmem, inspect.
fn open_document_impl(path: &str, password: &str) -> Result<ffi::OpenResultFFI, String> {
    use std::path::Path;

    let file_path = Path::new(path);
    if !file_path.exists() {
        return Err(format!("file not found: {path}"));
    }

    // Close any previous session first.
    close_document_impl();

    let doc_file = std::fs::File::open(file_path).map_err(|e| format!("open: {e}"))?;
    let region = SharedRegion::create(TILE_RGBA8_BYTES).map_err(|e| e.to_string())?;

    let exe = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .parent()
        .ok_or("no exe parent")?
        .join(if cfg!(windows) {
            "worker.exe"
        } else {
            "worker"
        });

    let pw = if password.is_empty() {
        None
    } else {
        Some(password)
    };

    let child = spawn_worker_with_attachments(
        &exe,
        &SpawnAttachments {
            doc: Some(&doc_file),
            shmem: Some(region.file()),
            output: None,
            password: pw,
        },
        &[],
    )
    .map_err(|e| format!("spawn worker ({}): {e}", exe.display()))?;


    let mut session = DocSession {
        child,
        region,
        next_cid: 1,
        page_count: 1,
        page_width: 595.0,
        page_height: 842.0,
        summary: StructuralSummary {
            page_count: 1,
            has_acroform: false,
            has_xfa: false,
            has_js: false,
            sig_count: 0,
            leniency_count: 0,
            leniency_events: vec![],
            page_dimensions: vec![],
            original_offsets: std::collections::HashMap::new(),
        },
        annotations: AnnotationStore::new(),
        form: AcroForm::new(),
        form_import_notes: Vec::new(),
        form_undo_stack: Vec::new(),
        form_redo_stack: Vec::new(),
        annot_undo_stack: Vec::new(),
        annot_redo_stack: Vec::new(),
        text_cache: TextCache::default(),
        path: path.to_string(),
    };

    match inspect_document(&mut session) {
        Ok(summary) => {
            let dims = summary.page_dimensions_f();
            let (w, h) = if let Some((w, h, _)) = dims.first() {
                (*w, *h)
            } else {
                (595.0, 842.0)
            };
            session.page_count = summary.page_count;
            session.page_width = w;
            session.page_height = h;
            session.summary = summary;
        }
        Err(e) => {
            // Password-required documents often fail inspect until password is correct.
            let msg = e.to_lowercase();
            if msg.contains("password") || msg.contains("encrypt") || msg.contains("security") {
                return Err(format!("password required: {e}"));
            }
            eprintln!("ffi-bridge: inspect failed, using defaults: {e}");
        }
    }

    // Import AcroForm fields from COS when the document declares them. [FR-FORM-1]
    load_form_from_path(&mut session);

    // Same-process shell: base pointer of Rust SharedRegion map (not a Win32 file HANDLE).
    // MapViewOfFile cannot take a plain file handle. [ADR-011, SDS Ã‚Â§6.3]
    let shmem_ptr = session.region.as_slice().as_ptr() as isize;

    let result = ffi::OpenResultFFI {
        page_count: session.page_count,
        page_width: session.page_width,
        page_height: session.page_height,
        shmem_handle: shmem_ptr,
        leniency_count: session.summary.leniency_count,
        has_acroform: session.summary.has_acroform,
        has_js: session.summary.has_js,
        has_xfa: session.summary.has_xfa,
        sig_count: session.summary.sig_count,
    };

    // SAFETY: single-threaded Qt main thread.
    unsafe {
        SESSION = Some(Box::new(session));
    }

    Ok(result)
}

fn inspect_document(session: &mut DocSession) -> Result<StructuralSummary, String> {
    let correlation_id = next_cid(session);
    let event = send_recv(session, Command::Inspect { correlation_id })?;
    match event {
        WorkerEvent::Summary {
            correlation_id: cid,
            summary,
        } if cid == correlation_id => Ok(summary),
        WorkerEvent::RenderError { message, .. } => Err(message),
        other => Err(format!("unexpected event: {other:?}")),
    }
}

fn render_tile_impl(
    page: u32,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    scale: f32,
    generation: u64,
) -> Result<ffi::TileResultFFI, String> {
    with_session_mut(|session| {
        let correlation_id = next_cid(session);
        let event = send_recv(
            session,
            Command::RenderTile {
                correlation_id,
                page,
                x,
                y,
                w,
                h,
                scale,
                generation,
                slot_offset: 0,
                col: 0,
                row: 0,
            },
        )?;
        match event {
            WorkerEvent::TileReady { desc, .. } => Ok(ffi::TileResultFFI {
                offset: desc.offset,
                len: desc.len,
                generation: desc.generation,
            }),
            WorkerEvent::RenderError { message, .. } => Err(message),
            _ => Err("unexpected event type".into()),
        }
    })
}

fn close_document_impl() {
    // SAFETY: single-threaded Qt main thread.
    if let Some(mut session) = unsafe { SESSION.take() } {
        // Typed quit — same path as protocol tests. [ADR-004, SDS §10]
        let body = encode_command(&Command::Quit);
        let _ = session.child.transport.send(&body);
        let _ = session.child.child.wait();
    }
}

fn diagnostics_impl() -> Result<String, String> {
    with_session_mut(|session| {
        let s = &session.summary;
        let mut out = String::new();
        out.push_str(&format!("Path: {}\n", session.path));
        out.push_str(&format!("Pages: {}\n", s.page_count));
        out.push_str(&format!("AcroForm: {}\n", s.has_acroform));
        out.push_str(&format!("JavaScript: {}\n", s.has_js));
        out.push_str(&format!("XFA: {}\n", s.has_xfa));
        out.push_str(&format!("Signatures: {}\n", s.sig_count));
        out.push_str(&format!("Leniency: {}\n", s.leniency_count));
        for e in &s.leniency_events {
            out.push_str(&format!("  - {e}\n"));
        }
        out.push_str(&format!(
            "Annotations (session): {}\n",
            session.annotations.all_annotations().len()
        ));
        out.push_str(&format!(
            "Form fields (session): {}\n",
            session.form.field_count()
        ));
        out.push_str(&format!(
            "Forms JS enabled: {}\n",
            session.form.javascript_enabled
        ));
        for n in &session.form_import_notes {
            out.push_str(&format!("Form import: {n}\n"));
        }
        out.push_str(&format!("Text cache pages: {}\n", session.text_cache.len()));
        Ok(out)
    })
}

fn leniency_events_impl() -> Result<String, String> {
    with_session_mut(|session| Ok(session.summary.leniency_events.join("\n")))
}

fn get_outline_impl() -> Result<String, String> {
    with_session_mut(|session| {
        let correlation_id = next_cid(session);
        let event = send_recv(session, Command::GetOutline { correlation_id })?;
        match event {
            WorkerEvent::OutlineResult {
                entry_count,
                total_count,
                data,
                ..
            } => Ok(format!(
                "entries={entry_count}\ntotal={total_count}\n{data}"
            )),
            WorkerEvent::RenderError { message, .. } => Err(message),
            other => Err(format!("unexpected: {other:?}")),
        }
    })
}

fn get_layers_impl() -> Result<String, String> {
    with_session_mut(|session| {
        let correlation_id = next_cid(session);
        let event = send_recv(session, Command::GetLayers { correlation_id })?;
        match event {
            WorkerEvent::LayersResult {
                group_count,
                total_count,
                has_layers,
                ..
            } => Ok(format!(
                "has_layers={has_layers}\ngroups={group_count}\ntotal={total_count}"
            )),
            WorkerEvent::RenderError { message, .. } => Err(message),
            other => Err(format!("unexpected: {other:?}")),
        }
    })
}

fn get_attachments_impl() -> Result<String, String> {
    with_session_mut(|session| {
        let correlation_id = next_cid(session);
        let event = send_recv(session, Command::GetAttachments { correlation_id })?;
        match event {
            WorkerEvent::AttachmentsResult { count, data, .. } => {
                Ok(format!("count={count}\n{data}"))
            }
            WorkerEvent::RenderError { message, .. } => Err(message),
            other => Err(format!("unexpected: {other:?}")),
        }
    })
}

fn ensure_page_text(session: &mut DocSession, page_index: u32) -> Result<(), String> {
    if session.text_cache.contains(page_index) {
        return Ok(());
    }
    let correlation_id = next_cid(session);
    let event = send_recv(
        session,
        Command::ExtractPage {
            correlation_id,
            page_index,
        },
    )?;
    match event {
        WorkerEvent::TextExtracted {
            full_text,
            reliable,
            line_geom,
            page_index: pi,
            ..
        } if pi == page_index => {
            session.text_cache.insert(
                page_index,
                CachedPageText {
                    full_text,
                    reliable,
                    line_geom,
                },
            );
            Ok(())
        }
        WorkerEvent::RenderError { message, .. } => Err(message),
        other => Err(format!("unexpected extract: {other:?}")),
    }
}

fn extract_page_text_impl(page_index: u32) -> Result<String, String> {
    with_session_mut(|session| {
        ensure_page_text(session, page_index)?;
        let cached = session
            .text_cache
            .get(page_index)
            .ok_or("text cache miss")?;
        Ok(format!(
            "reliable={}\n{}",
            cached.reliable, cached.full_text
        ))
    })
}

fn find_text_impl(query: &str) -> Result<String, String> {
    with_session_mut(|session| {
        if query.is_empty() {
            return Ok(String::new());
        }
        let n = session.page_count;
        let mut lines_out = Vec::new();
        let mut total = 0u32;
        for page in 0..n {
            ensure_page_text(session, page)?;
            let cached = session.text_cache.get(page).expect("just extracted");
            // Build a minimal PageTextModel for search crate
            let model = geom_to_model(page, cached);
            let opts = FindOptions::default();
            let matches = find_all(&model, query, &opts);
            for m in matches {
                total += 1;
                lines_out.push(format!(
                    "hit page={} line={} offset={} len={} x={:.1} y={:.1} w={:.1} h={:.1} text={:?} reliable={}",
                    m.page_index,
                    m.line_index,
                    m.char_offset,
                    m.char_len,
                    m.x,
                    m.y,
                    m.width,
                    m.height,
                    m.matched_text,
                    cached.reliable
                ));
            }
        }
        Ok(format!("total={total}\n{}", lines_out.join("\n")))
    })
}

fn geom_to_model(page: u32, cached: &CachedPageText) -> engine_api::extract::PageTextModel {
    use engine_api::extract::{PageTextModel, TextLine};
    let lines: Vec<TextLine> = if !cached.line_geom.is_empty() {
        cached
            .line_geom
            .iter()
            .filter_map(|g| {
                let parts: Vec<&str> = g.splitn(6, '|').collect();
                if parts.len() < 6 {
                    return None;
                }
                Some(TextLine {
                    index: parts[0].parse().ok()?,
                    x: parts[1].parse().ok()?,
                    y: parts[2].parse().ok()?,
                    width: parts[3].parse().ok()?,
                    height: parts[4].parse().ok()?,
                    text: parts[5]
                        .replace("\\n", "\n")
                        .replace("\\p", "|")
                        .replace("\\\\", "\\"),
                    spans: vec![],
                })
            })
            .collect()
    } else {
        cached
            .full_text
            .lines()
            .enumerate()
            .map(|(i, t)| TextLine {
                index: i as u32,
                text: t.to_string(),
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 12.0,
                spans: vec![],
            })
            .collect()
    };
    let char_count = lines.iter().map(|l| l.text.len() as u32).sum();
    PageTextModel {
        page_index: page,
        lines,
        reliable: cached.reliable,
        char_count,
        has_structure: false,
    }
}

fn add_annotation_impl(
    page_index: u32,
    annot_type: &str,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    contents: &str,
) -> Result<u64, String> {
    with_session_mut(|session| {
        let id = session.annotations.next_id();
        let ty = match annot_type {
            "highlight" => AnnotationType::TextMarkup(TextMarkupKind::Highlight),
            "underline" => AnnotationType::TextMarkup(TextMarkupKind::Underline),
            "strikeout" => AnnotationType::TextMarkup(TextMarkupKind::Strikeout),
            "squiggly" => AnnotationType::TextMarkup(TextMarkupKind::Squiggly),
            "note" | "sticky" => AnnotationType::StickyNote,
            "freetext" | "text" => AnnotationType::FreeText,
            "ink" => AnnotationType::Ink,
            "rect" | "rectangle" => AnnotationType::Rectangle,
            "ellipse" | "circle" => AnnotationType::Ellipse,
            "stamp" => AnnotationType::Stamp,
            "redact" => AnnotationType::Redaction,
            _ => return Err(format!("unknown annotation type: {annot_type}")),
        };
        let mut ann = Annotation::new(id, page_index, ty, Rect::new(x, y, w, h))
            .with_contents(contents)
            .with_author("user");
        // Default highlight color
        if matches!(ty, AnnotationType::TextMarkup(TextMarkupKind::Highlight)) {
            ann.properties.color = Color {
                r: 1.0,
                g: 1.0,
                b: 0.0,
                a: 0.4,
            };
        }
        // QuadPoints for text markup covering the rect. [FR-ANNOT-3]
        if matches!(ty, AnnotationType::TextMarkup(_)) {
            use pdf_model::annotation::QuadPoints;
            ann.quad_points = Some(QuadPoints::from_rect(&ann.rect));
        }
        ann.ensure_appearance();
        debug_assert!(ann.has_appearance());
        let _ = generate_appearance(&ann); // ensure generators stay warm
        session.annotations.page_mut(page_index).add(ann.clone());
        // Track for undo *with the data*, so redo can restore it.
        // [FR-ANNOT-4, M4]
        session
            .annot_undo_stack
            .push(AnnotUndoEntry::Created { annotation: ann, page_index });
        session.annot_redo_stack.clear();
        Ok(id)
    })
}

/// Delete an annotation by ID. [FR-ANNOT-4, M4]
fn delete_annotation_impl(annotation_id: u64) -> Result<String, String> {
    with_session_mut(|session| {
        let ann = session.annotations.find(annotation_id)
            .ok_or(format!("annotation {annotation_id} not found"))?;
        let page_index = ann.page_index;
        let saved = ann.clone();
        session.annotations.page_mut(page_index).remove(annotation_id);
        // Track for undo. [FR-ANNOT-4]
        session.annot_undo_stack.push(AnnotUndoEntry::Deleted { annotation: saved, page_index });
        session.annot_redo_stack.clear();
        Ok(format!("deleted annotation {annotation_id}"))
    })
}

/// Undo the last annotation or form operation. [FR-ANNOT-4, FR-FORM-6, M4]
fn undo_impl() -> Result<String, String> {
    with_session_mut(|session| {
        // Try annotation undo first.
        if let Some(entry) = session.annot_undo_stack.pop() {
            let msg = match &entry {
                AnnotUndoEntry::Created { annotation, page_index } => {
                    session.annotations.page_mut(*page_index).remove(annotation.id);
                    format!("undid create annotation {}", annotation.id)
                }
                AnnotUndoEntry::Deleted { annotation, .. } => {
                    let page = annotation.page_index;
                    session.annotations.page_mut(page).add(annotation.clone());
                    format!("undid delete annotation {}", annotation.id)
                }
            };
            session.annot_redo_stack.push(entry);
            return Ok(msg);
        }
        // Fall back to form undo.
        form_undo_impl()
    })
}

/// Redo the last undone annotation or form operation. [FR-ANNOT-4, FR-FORM-6, M4]
fn redo_impl() -> Result<String, String> {
    with_session_mut(|session| {
        // Try annotation redo first.
        if let Some(entry) = session.annot_redo_stack.pop() {
            let msg = match &entry {
                AnnotUndoEntry::Created { annotation, page_index } => {
                    // Redo of a creation re-creates it. This used to return
                    // Ok("redo not available for create N"): a success the shell
                    // showed as a status message while the annotation stayed
                    // gone, with the entry pushed back onto the undo stack so
                    // the two stacks disagreed about reality. [FR-ANNOT-4, GR-8]
                    session
                        .annotations
                        .page_mut(*page_index)
                        .add(annotation.clone());
                    format!("redid create annotation {}", annotation.id)
                }
                AnnotUndoEntry::Deleted { annotation, .. } => {
                    let page = annotation.page_index;
                    session.annotations.page_mut(page).add(annotation.clone());
                    format!("redid delete annotation {}", annotation.id)
                }
            };
            session.annot_undo_stack.push(entry);
            return Ok(msg);
        }
        // Fall back to form redo.
        form_redo_impl()
    })
}

/// Whether undo is available. [FR-ANNOT-4, FR-FORM-6, M4]
fn can_undo_impl() -> bool {
    with_session_mut(|session| {
        Ok(!session.annot_undo_stack.is_empty() || !session.form_undo_stack.is_empty())
    }).unwrap_or(false)
}

/// Whether redo is available. [FR-ANNOT-4, FR-FORM-6, M4]
fn can_redo_impl() -> bool {
    with_session_mut(|session| {
        Ok(!session.annot_redo_stack.is_empty() || !session.form_redo_stack.is_empty())
    }).unwrap_or(false)
}

fn export_xfdf_impl() -> Result<String, String> {
    with_session_mut(|session| Ok(export_xfdf(&session.annotations, None)))
}

fn import_xfdf_impl(xml: &str) -> Result<u32, String> {
    with_session_mut(|session| {
        let n = import_xfdf_to_store(xml, &mut session.annotations);
        Ok(n as u32)
    })
}

fn annotation_count_impl() -> Result<u32, String> {
    with_session_mut(|session| Ok(session.annotations.all_annotations().len() as u32))
}

/// Flatten all form fields into page content. [FR-FORM-4, M5]
fn flatten_form_impl() -> Result<String, String> {
    with_session_mut(|session| {
        if session.form.field_count() == 0 {
            return Ok("no form fields to flatten".into());
        }
        let results = pdf_model::form::flatten_form(&session.form);
        let count = results.len();
        session.form = pdf_model::form::AcroForm::new();
        Ok(format!("flattened {count} fields"))
    })
}

/// Get annotations for a page as renderable data. [FR-ANNOT, M4]
///
/// Returns lines: "id=T|x=Y|y=Y|w=W|h=H|type=T|contents=C|color=R,G,B,A"
/// for each annotation on the page.
fn get_page_annotations_impl(page_index: u32) -> Result<String, String> {
    with_session_mut(|session| {
        let page = session.annotations.page(page_index);
        let annotations = match page {
            Some(p) => &p.annotations,
            None => return Ok(String::new()),
        };
        let mut lines = Vec::new();
        for ann in annotations {
            let c = &ann.properties.color;
            lines.push(format!(
                "id={}|x={}|y={}|w={}|h={}|type={}|contents={}|color={},{},{},{}",
                ann.id,
                ann.rect.x, ann.rect.y, ann.rect.width, ann.rect.height,
                ann.pdf_type_str(),
                ann.properties.contents.replace('|', "\\p").replace('\n', "\\n"),
                c.r, c.g, c.b, c.a,
            ));
        }
        Ok(lines.join("\n"))
    })
}


fn get_object_bytes(session: &mut DocSession, obj_num: u32) -> Result<Vec<u8>, String> {
    let correlation_id = next_cid(session);
    let event = send_recv(session, Command::GetObject { correlation_id, obj_num })?;
    match event {
        WorkerEvent::ObjectData { data, obj_num: on, .. } if on == obj_num => Ok(data),
        WorkerEvent::RenderError { message, .. } => Err(message),
        other => Err(format!("unexpected get_object: {other:?}")),
    }
}

/// Persist annotations into an incremental PDF update. [FR-ANNOT-4, ADR-012]
fn save_document_impl(out_path: &str) -> Result<String, String> {
    with_session_mut(|session| {
        let original = std::fs::read(&session.path).map_err(|e| format!("read source: {e}"))?;
        let xfdf = export_xfdf(&session.annotations, None);

        let xfdf_path = std::path::Path::new(out_path).with_extension("xfdf");
        std::fs::write(&xfdf_path, xfdf.as_bytes()).map_err(|e| format!("xfdf write: {e}"))?;

        if session.annotations.all_annotations().is_empty() && session.form.field_count() == 0 {
            if out_path != session.path {
                std::fs::write(out_path, &original).map_err(|e| format!("copy: {e}"))?;
            }
            return Ok(format!(
                "saved without annots/forms; xfdf={}",
                xfdf_path.display()
            ));
        }

        let mut overlay = CowOverlay::new();
        let mut next_obj = session
            .summary
            .original_offsets
            .keys()
            .copied()
            .max()
            .unwrap_or(10)
            + 1;

        use std::collections::HashMap;
        let mut by_page: HashMap<u32, Vec<pdf_model::annotation::Annotation>> = HashMap::new();
        for a in session.annotations.all_annotations() {
            by_page.entry(a.page_index).or_default().push(a.clone());
        }

        let page_objs: Vec<u32> = {
            let mut objs = Vec::new();
            if let Ok(pages) = get_object_bytes(session, 2) {
                let text = String::from_utf8_lossy(&pages);
                if let Some(k) = text.find("/Kids") {
                    let slice = &text[k..];
                    if let Some(lb) = slice.find('[') {
                        if let Some(rb) = slice[lb..].find(']') {
                            let arr = &slice[lb + 1..lb + rb];
                            for tok in arr.split_whitespace() {
                                if let Ok(n) = tok.parse::<u32>() {
                                    if n > 0 && !objs.contains(&n) {
                                        objs.push(n);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if objs.is_empty() {
                objs = (0..session.page_count).map(|i| i + 3).collect();
            }
            objs
        };

        for (page_index, anns) in &by_page {
            let page_obj_num = *page_objs.get(*page_index as usize).ok_or_else(|| {
                format!("no page object for page {page_index}")
            })?;
            let original_page = get_object_bytes(session, page_obj_num).unwrap_or_else(|_| {
                format!(
                    "{page_obj_num} 0 obj\n<< /Type /Page /MediaBox [0 0 {} {}] >>\nendobj\n",
                    session.page_width, session.page_height
                )
                .into_bytes()
            });

            let mut annot_nums = Vec::new();
            for ann in anns {
                let mut a = ann.clone();
                let objs = build_annotation_pdf_objects(&mut a, next_obj, next_obj + 1);
                overlay.set_object(objs.annot_obj_num, objs.annot_bytes);
                overlay.set_object(objs.ap_obj_num, objs.ap_bytes);
                annot_nums.push(objs.annot_obj_num);
                next_obj += 2;
            }
            let patched = inject_annot_refs(&original_page, &annot_nums)
                .map_err(|e| format!("page patch: {e}"))?;
            overlay.set_object(page_obj_num, patched);
        }

        // Form widgets with always-written /AP. [FR-FORM-1, M5]
        let mut form_widget_count = 0u32;
        if session.form.field_count() > 0 {
            if session.form.needs_appearance_regen {
                session.form.regenerate_appearances();
            }
            use std::collections::HashMap as HMap;
            let mut form_by_page: HMap<u32, Vec<String>> = HMap::new();
            for f in session.form.fields_in_tab_order() {
                form_by_page
                    .entry(f.page_index)
                    .or_default()
                    .push(f.fully_qualified_name.clone());
            }
            for (page_index, names) in form_by_page {
                let page_obj_num = *page_objs.get(page_index as usize).ok_or_else(|| {
                    format!("no page object for form page {page_index}")
                })?;
                let original_page = overlay
                    .get_object(page_obj_num)
                    .map(|s| s.to_vec())
                    .or_else(|| get_object_bytes(session, page_obj_num).ok())
                    .unwrap_or_else(|| {
                        format!(
                            "{page_obj_num} 0 obj\n<< /Type /Page /MediaBox [0 0 {} {}] >>\nendobj\n",
                            session.page_width, session.page_height
                        )
                        .into_bytes()
                    });
                let mut widget_nums = Vec::new();
                for name in names {
                    if let Some(field) = session.form.field_mut(&name) {
                        // Reuse the document's own widget object number when the
                        // field came from the file.
                        //
                        // Allocating a fresh one appended a *second* widget with
                        // the same /T: the page ended up with `/Annots [6 0 R
                        // 5 0 R]`, the original still carrying `/V ()`, and
                        // `/AcroForm /Fields` still pointing at that original.
                        // So the typed value was in the file and the form still
                        // read empty — and a reader could legitimately show
                        // either widget. [FR-FORM-1, FR-FORM-4, PRIN-1, GR-8]
                        let existing_widget = field.widget_obj_num;
                        let widget_num = existing_widget.unwrap_or_else(|| {
                            let n = next_obj;
                            next_obj += 1;
                            n
                        });
                        let ap_num = next_obj;
                        next_obj += 1;

                        let objs = build_widget_pdf_objects(field, widget_num, ap_num);
                        overlay.set_object(objs.widget_obj_num, objs.widget_bytes);
                        overlay.set_object(objs.ap_obj_num, objs.ap_bytes);
                        // An existing widget is already in the page's /Annots;
                        // adding it again would duplicate the reference.
                        if existing_widget.is_none() {
                            widget_nums.push(objs.widget_obj_num);
                        }
                        form_widget_count += 1;
                    }
                }
                if !widget_nums.is_empty() {
                    let patched = inject_annot_refs(&original_page, &widget_nums)
                        .map_err(|e| format!("form page patch: {e}"))?;
                    overlay.set_object(page_obj_num, patched);
                }
            }
        }

        let prev_xref = {
            let text = String::from_utf8_lossy(&original);
            text.rfind("startxref")
                .and_then(|i| {
                    text[i + 9..]
                        .lines()
                        .find(|l| !l.trim().is_empty())
                        .and_then(|l| l.trim().parse::<u32>().ok())
                })
                .unwrap_or(0)
        };

        let mut out = original.clone();
        let original_len = out.len() as u32;
        let result = IncrementalWriter::write_incremental(
            &mut out,
            &overlay,
            prev_xref,
            next_obj,
            &session.summary.original_offsets,
            original_len,
        )
        .map_err(|e| format!("incremental write: {e}"))?;

        std::fs::write(out_path, &out).map_err(|e| format!("write out: {e}"))?;
        Ok(format!(
            "saved {} annots, {} form widgets, {} objects, xfdf={}",
            session.annotations.all_annotations().len(),
            form_widget_count,
            result.objects_written,
            xfdf_path.display()
        ))
    })
}


/// Load session form from document bytes (COS AcroForm walk). [FR-FORM-1]
fn load_form_from_path(session: &mut DocSession) {
    session.form_import_notes.clear();
    match std::fs::read(&session.path) {
        Ok(bytes) => match import_acroform_from_bytes(&bytes) {
            Ok(imported) => {
                session.form_import_notes = imported.notes;
                if imported.field_count > 0 {
                    session.form = imported.form;
                    session.form_import_notes.push(format!(
                        "imported {} fields from document AcroForm",
                        imported.field_count
                    ));
                } else if session.summary.has_acroform {
                    session.form_import_notes.push(
                        "document has AcroForm but no leaf fields parsed (Kids/compressed unsupported?)"
                            .into(),
                    );
                }
            }
            Err(e) => {
                session
                    .form_import_notes
                    .push(format!("form import failed: {e}"));
            }
        },
        Err(e) => {
            session
                .form_import_notes
                .push(format!("form import read failed: {e}"));
        }
    }
}

/// Seed a session form with a simple SUM calc demo. [FR-FORM-1, FR-JS-1, M5]
fn seed_demo_form(form: &mut AcroForm, page_height: f32) {
    *form = AcroForm::new();
    form.has_javascript = true;
    form.javascript_enabled = true;

    let y0 = (page_height - 120.0).max(72.0);
    let mut a = FormField::new("a", FieldType::Text, 0, FieldRect::new(72.0, y0, 80.0, 18.0));
    a.tab_order = 1;
    a.tooltip = "Addend a".into();
    a.set_value(FieldValue::Text("10".into()));
    form.add_field(a);

    let mut b = FormField::new("b", FieldType::Text, 0, FieldRect::new(72.0, y0 - 28.0, 80.0, 18.0));
    b.tab_order = 2;
    b.tooltip = "Addend b".into();
    b.set_value(FieldValue::Text("5".into()));
    form.add_field(b);

    let mut total =
        FormField::new("total", FieldType::Text, 0, FieldRect::new(72.0, y0 - 56.0, 80.0, 18.0));
    total.tab_order = 3;
    total.tooltip = "Sum (calculated)".into();
    total.read_only = true;
    total.calculation = Some(FieldCalculation {
        expression: r#"AFSimple_Calculate("SUM", ["a","b"])"#.into(),
        dependencies: vec!["a".into(), "b".into()],
        enabled: true,
    });
    form.add_field(total);
    form.calculation_order = vec!["total".into()];
    form.regenerate_appearances();
}

/// List session form fields (tab order). [FR-FORM, M5]
fn list_form_fields_impl() -> Result<String, String> {
    with_session_mut(|session| {
        if session.form.field_count() == 0 {
            let note = if session.summary.has_acroform {
                "count=0\nnote=AcroForm present but no leaf fields imported; try Seed demo or check Kids/xref"
            } else {
                "count=0\nnote=no AcroForm in document; Seed demo for local fill model"
            };
            let mut out = note.to_string();
            for n in &session.form_import_notes {
                out.push_str(&format!("\nimport:{n}"));
            }
            out.push('\n');
            return Ok(out);
        }
        let mut lines = Vec::new();
        lines.push(format!(
            "count={}\nhas_js={}\njs_enabled={}\nneeds_ap={}\nnote=COS import + session fill model\n",
            session.form.field_count(),
            session.form.has_javascript || session.form.detect_javascript(),
            session.form.javascript_enabled,
            session.form.needs_appearance_regen,
        ));
        for n in &session.form_import_notes {
            lines.push(format!("import:{n}"));
        }
        for f in session.form.fields_in_tab_order() {
            let ap = if f.appearance.is_some() { "yes" } else { "no" };
            let ro = if f.read_only { "ro" } else { "rw" };
            let calc = if f.calculation.is_some() { "calc" } else { "-" };
            lines.push(format!(
                "{}\t{}\t{}\t{}\t{}\tap={}",
                f.fully_qualified_name,
                f.pdf_type_str(),
                f.value.display(),
                ro,
                calc,
                ap
            ));
        }
        Ok(lines.join("\n"))
    })
}

/// Seed the session form with the demo calc set. [FR-FORM, FR-JS, M5]
fn seed_form_demo_impl() -> Result<String, String> {
    with_session_mut(|session| {
        seed_demo_form(&mut session.form, session.page_height);
        session.form_import_notes = vec!["seeded demo form (replaced COS import)".into()];
        Ok(format!(
            "seeded {} fields (a,b,total SUM); appearances regenerated",
            session.form.field_count()
        ))
    })
}

/// Re-run COS AcroForm import from the open file. [FR-FORM-1]
fn reload_form_from_document_impl() -> Result<String, String> {
    with_session_mut(|session| {
        load_form_from_path(session);
        Ok(format!(
            "reloaded {} fields; notes={:?}",
            session.form.field_count(),
            session.form_import_notes
        ))
    })
}

/// Set a session form field value and regenerate its appearance. [FR-FORM-1]
fn set_form_field_impl(name: &str, value: &str) -> Result<String, String> {
    with_session_mut(|session| {
        if session.form.field_count() == 0 {
            // The demo model exists so a document *without* a form can still
            // exercise the fill path. Seeding it for a document that HAS an
            // AcroForm we failed to import would put invented fields in front
            // of a user editing a real form, and any value typed into them
            // would be written back into that document. Refuse and say why.
            // [FR-FORM-1, GR-8, PRIN-6]
            if session.summary.has_acroform {
                return Err(format!(
                    "this document has an AcroForm whose fields could not be imported, so \
                     there is no field '{name}' to set; filling a demo field here would \
                     write invented data into a real form"
                ));
            }
            seed_demo_form(&mut session.form, session.page_height);
        }
        let Some(field) = session.form.field(name) else {
            return Err(format!("unknown field: {name}"));
        };
        if field.read_only && field.calculation.is_some() {
            return Err(format!(
                "field {name} is calculation-driven; edit dependencies and run_forms_calc"
            ));
        }
        let new_value = match field.field_type {
            FieldType::Checkbox => FieldValue::Bool(
                value.eq_ignore_ascii_case("yes")
                    || value.eq_ignore_ascii_case("true")
                    || value == "1"
                    || value.eq_ignore_ascii_case("on"),
            ),
            FieldType::RadioButton | FieldType::ComboBox | FieldType::ListBox => {
                FieldValue::Choice(value.to_string())
            }
            _ => FieldValue::Text(value.to_string()),
        };
        let old_value = field.value.clone();
        let changed = session.form.set_field_value(name, new_value.clone());
        if changed {
            // Record undo entry and clear redo stack. [FR-FORM-6]
            session.form_undo_stack.push(FormUndoEntry {
                field_name: name.to_string(),
                old_value,
                new_value,
            });
            session.form_redo_stack.clear();
            session.form.regenerate_appearances();
        }
        let ap = session
            .form
            .field(name)
            .and_then(|f| f.appearance.as_ref())
            .map(|b| b.len())
            .unwrap_or(0);
        Ok(format!(
            "field={name} changed={changed} ap_bytes={ap} value={}",
            session
                .form
                .field(name)
                .map(|f| f.value.display())
                .unwrap_or_default()
        ))
    })
}

/// Undo the last form field change. [FR-FORM-6]
fn form_undo_impl() -> Result<String, String> {
    with_session_mut(|session| {
        let entry = session.form_undo_stack.pop().ok_or("nothing to undo")?;
        // Apply the old value back.
        let changed = session.form.set_field_value(&entry.field_name, entry.old_value.clone());
        if changed {
            session.form.regenerate_appearances();
            session.form_redo_stack.push(entry.clone());
        }
        Ok(format!(
            "undid {} value={}",
            entry.field_name,
            session.form.field(&entry.field_name)
                .map(|f| f.value.display())
                .unwrap_or_default()
        ))
    })
}

/// Redo the last undone form field change. [FR-FORM-6]
fn form_redo_impl() -> Result<String, String> {
    with_session_mut(|session| {
        let entry = session.form_redo_stack.pop().ok_or("nothing to redo")?;
        let changed = session.form.set_field_value(&entry.field_name, entry.new_value.clone());
        if changed {
            session.form.regenerate_appearances();
            session.form_undo_stack.push(entry.clone());
        }
        Ok(format!(
            "redid {} value={}",
            entry.field_name,
            session.form.field(&entry.field_name)
                .map(|f| f.value.display())
                .unwrap_or_default()
        ))
    })
}

/// Whether form undo is available. [FR-FORM-6]
fn form_can_undo_impl() -> bool {
    with_session_mut(|session| Ok(!session.form_undo_stack.is_empty())).unwrap_or(false)
}

/// Whether form redo is available. [FR-FORM-6]
fn form_can_redo_impl() -> bool {
    with_session_mut(|session| Ok(!session.form_redo_stack.is_empty())).unwrap_or(false)
}

/// Name of the next undoable form action. [FR-FORM-6]
fn form_undo_name_impl() -> String {
    with_session_mut(|session| {
        Ok(session.form_undo_stack.last()
            .map(|e| format!("Fill {}: {} -> {}", e.field_name, e.old_value.display(), e.new_value.display()))
            .unwrap_or_default())
    }).unwrap_or_default()
}

/// Validate all form fields and return errors. [FR-FORM-2]
fn validate_form_impl() -> Result<String, String> {
    with_session_mut(|session| {
        let errors = session.form.validate_all();
        if errors.is_empty() {
            Ok("valid".into())
        } else {
            Ok(format!("errors={}", errors.join("; ")))
        }
    })
}

/// Run forms JS subset calculations and regenerate appearances. [FR-JS-1, FR-FORM-1]
fn run_forms_calc_impl() -> Result<String, String> {
    with_session_mut(|session| {
        if session.form.field_count() == 0 {
            seed_demo_form(&mut session.form, session.page_height);
        }
        let result = run_form_calculations(&mut session.form);
        // Always regen AP after calc path (values may have changed).
        let ap_n = session.form.regenerate_appearances();
        let mut out = format!(
            "updated={:?}\nap_fields={ap_n}\njs_enabled={}\n",
            result.updated_fields, session.form.javascript_enabled
        );
        for name in &result.updated_fields {
            if let Some(f) = session.form.field(name) {
                out.push_str(&format!(
                    "  {name}={} ap={}\n",
                    f.value.display(),
                    f.appearance.is_some()
                ));
            }
        }
        for e in &result.log {
            out.push_str(&format!(
                "log: [{}] {}\n",
                if e.unsupported { "unsupported" } else { "info" },
                e.detail
            ));
        }
        Ok(out)
    })
}

/// Toggle or query forms JS kill switch. [FR-JS-4]
fn set_forms_js_enabled_impl(enabled: bool) -> Result<String, String> {
    with_session_mut(|session| {
        session.form.javascript_enabled = enabled;
        Ok(format!("forms_js_enabled={enabled}"))
    })
}

fn page_count_impl() -> Result<u32, String> {
    with_session_mut(|session| Ok(session.page_count))
}

#[cxx::bridge(namespace = "pdf_platform")]
mod ffi {
    /// Result of opening a document.
    struct OpenResultFFI {
        page_count: u32,
        page_width: f32,
        page_height: f32,
        shmem_handle: isize,
        leniency_count: u32,
        has_acroform: bool,
        has_js: bool,
        has_xfa: bool,
        sig_count: u32,
    }

    /// Tile descriptor returned after rendering.
    struct TileResultFFI {
        offset: u32,
        len: u32,
        generation: u64,
    }

    extern "Rust" {
        /// Open document; pass empty password if none. [FR-VIEW, encrypt]
        fn open_document_impl(path: &str, password: &str) -> Result<OpenResultFFI>;
        fn render_tile_impl(
            page: u32,
            x: u32,
            y: u32,
            w: u32,
            h: u32,
            scale: f32,
            generation: u64,
        ) -> Result<TileResultFFI>;
        fn close_document_impl();
        fn page_count_impl() -> Result<u32>;
        fn diagnostics_impl() -> Result<String>;
        fn leniency_events_impl() -> Result<String>;
        fn get_outline_impl() -> Result<String>;
        fn get_layers_impl() -> Result<String>;
        fn get_attachments_impl() -> Result<String>;
        fn extract_page_text_impl(page_index: u32) -> Result<String>;
        fn find_text_impl(query: &str) -> Result<String>;
        fn add_annotation_impl(
            page_index: u32,
            annot_type: &str,
            x: f32,
            y: f32,
            w: f32,
            h: f32,
            contents: &str,
        ) -> Result<u64>;
        fn export_xfdf_impl() -> Result<String>;
        fn import_xfdf_impl(xml: &str) -> Result<u32>;
        fn annotation_count_impl() -> Result<u32>;
        fn get_page_annotations_impl(page_index: u32) -> Result<String>;
        fn delete_annotation_impl(annotation_id: u64) -> Result<String>;
        fn undo_impl() -> Result<String>;
        fn redo_impl() -> Result<String>;
        fn can_undo_impl() -> bool;
        fn can_redo_impl() -> bool;
        fn save_document_impl(out_path: &str) -> Result<String>;
        /// List session form fields (tab order). [FR-FORM, M5]
        fn list_form_fields_impl() -> Result<String>;
        /// Seed demo form (a,b,total SUM) for fill/calc/AP. [FR-FORM, FR-JS]
        fn seed_form_demo_impl() -> Result<String>;
        /// Re-import AcroForm fields from the open document. [FR-FORM-1]
        fn reload_form_from_document_impl() -> Result<String>;
        /// Set field value; regenerates widget /AP. [FR-FORM-1]
        fn set_form_field_impl(name: &str, value: &str) -> Result<String>;
        /// Run forms JS subset + regenerate appearances. [FR-JS-1, FR-FORM-1]
        fn run_forms_calc_impl() -> Result<String>;
        /// Forms JS kill switch. [FR-JS-4]
        fn set_forms_js_enabled_impl(enabled: bool) -> Result<String>;
        /// Undo last form field change. [FR-FORM-6]
        fn form_undo_impl() -> Result<String>;
        /// Redo last undone form change. [FR-FORM-6]
        fn form_redo_impl() -> Result<String>;
        /// Whether form undo is available. [FR-FORM-6]
        fn form_can_undo_impl() -> bool;
        /// Whether form redo is available. [FR-FORM-6]
        fn form_can_redo_impl() -> bool;
        /// Name of next undoable form action. [FR-FORM-6]
        fn form_undo_name_impl() -> String;
        /// Validate all form fields and return errors. [FR-FORM-2]
        fn validate_form_impl() -> Result<String>;
        /// Flatten all form fields into page content. [FR-FORM-4, M5]
        fn flatten_form_impl() -> Result<String>;
    }
}

#[cfg(test)]
mod text_cache_tests {
    use super::{CachedPageText, TextCache, TEXT_CACHE_MAX_BYTES};

    fn page_of(bytes: usize) -> CachedPageText {
        CachedPageText {
            full_text: "x".repeat(bytes),
            reliable: true,
            line_geom: Vec::new(),
        }
    }

    #[test]
    fn the_cache_stops_growing_at_its_budget() {
        // `find_text` extracts every page, so this used to hold an entire
        // document's text with nothing ever evicting it. [GR-7]
        let mut cache = TextCache::default();
        let page_size = TEXT_CACHE_MAX_BYTES / 4;

        for page in 0..20 {
            cache.insert(page, page_of(page_size));
        }

        assert!(
            cache.bytes <= TEXT_CACHE_MAX_BYTES,
            "cache holds {} bytes, over its {TEXT_CACHE_MAX_BYTES}-byte budget",
            cache.bytes
        );
        assert!(cache.len() < 20, "nothing was evicted");
        assert!(cache.contains(19), "the most recent page must survive");
        assert!(!cache.contains(0), "the oldest page should have gone first");
    }

    #[test]
    fn reading_a_page_makes_it_recent() {
        let mut cache = TextCache::default();
        // Quarter-budget pages: two fit comfortably, four do not.
        let page_size = TEXT_CACHE_MAX_BYTES / 4;
        cache.insert(0, page_of(page_size));
        cache.insert(1, page_of(page_size));

        // Touch page 0, then push until something must go.
        assert!(cache.get(0).is_some());
        cache.insert(2, page_of(page_size));
        cache.insert(3, page_of(page_size));

        assert!(
            cache.contains(0),
            "page 0 was read most recently of the two and must outlive page 1"
        );
        assert!(!cache.contains(1), "page 1 was the least recently used");
    }

    #[test]
    fn a_page_larger_than_the_whole_budget_is_still_returned() {
        // Otherwise the caller that just asked for it re-extracts forever.
        let mut cache = TextCache::default();
        cache.insert(5, page_of(TEXT_CACHE_MAX_BYTES * 2));

        assert!(cache.contains(5));
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn re_inserting_a_page_does_not_double_count_it() {
        let mut cache = TextCache::default();
        cache.insert(1, page_of(1000));
        let after_first = cache.bytes;
        cache.insert(1, page_of(1000));

        assert_eq!(cache.bytes, after_first, "the replaced entry was counted twice");
        assert_eq!(cache.len(), 1);
    }
}

#[cfg(test)]
mod bridge_lifecycle_tests {
    //! The bridge is the single boundary between the Qt shell and everything
    //! this product does (ADR-004, FFI-1), and it had **no tests at all**. The
    //! shell's QTests cover input translation and one tile render; every other
    //! user action — annotate, undo, export, save — crossed untested code.
    //!
    //! These drive the real `*_impl` functions the way the shell does, and
    //! assert outcomes rather than absence of error: an annotation must reach
    //! the saved PDF's page `/Annots`, not merely be accepted by the call.
    //!
    //! One test, run serially on purpose: the bridge owns a process-wide
    //! session, so splitting these into separate `#[test]` functions would let
    //! cargo run them concurrently against the same global state.

    use super::*;

    /// Serializes these tests against the bridge's process-wide session.
    ///
    /// The bridge deliberately holds one document at a time in a global, which
    /// is right for a single-window shell driven from the Qt main thread. Cargo
    /// runs tests in threads of one process, so two of them would drive the
    /// same session at once — they did, and the second failed on state the
    /// first had closed.
    static SESSION_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn exclusive_session() -> std::sync::MutexGuard<'static, ()> {
        SESSION_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn fixture() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tools/corpus-diff/fixtures/valid-1page.pdf")
    }

    /// Put the worker where the bridge looks for it: beside the running
    /// executable.
    ///
    /// That rule is right for the shipped app, where the worker sits next to
    /// the binary. For a lib test the running executable is in `target/*/deps`,
    /// and cargo only places the worker there on some platforms — Windows
    /// found it, Linux and macOS did not. Staging a copy keeps the product's
    /// resolution untouched. [SDS §3.1]
    fn stage_worker_beside_test_executable() -> std::path::PathBuf {
        let name = if cfg!(windows) { "worker.exe" } else { "worker" };
        let exe = std::env::current_exe().expect("current exe");
        let test_dir = exe.parent().expect("exe dir").to_path_buf();
        let staged = test_dir.join(name);
        if staged.is_file() {
            return staged;
        }
        // The build output directory is the parent of `deps`.
        let build_dir = if test_dir.ends_with("deps") {
            test_dir.parent().expect("deps parent").to_path_buf()
        } else {
            test_dir.clone()
        };
        let built = build_dir.join(name);
        assert!(
            built.is_file(),
            "worker binary not built at {}; run `cargo build -p worker-main`",
            built.display()
        );
        std::fs::copy(&built, &staged).expect("stage worker beside the test executable");
        staged
    }

    /// A one-page document carrying a real AcroForm text field.
    ///
    /// `valid-1page.pdf` has no form, and the bridge falls back to a seeded
    /// demo model when a document has none — which would make a form test pass
    /// without a form ever being read from or written to a file.
    fn acroform_pdf() -> Vec<u8> {
        let objects: Vec<Vec<u8>> = vec![
            b"<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [5 0 R] >> >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
               /Resources << /Font << /Helv 4 0 R >> >> /Annots [5 0 R] >>"
                .to_vec(),
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
            b"<< /Type /Annot /Subtype /Widget /FT /Tx /T (claimant) /V () \
               /Rect [72 700 372 724] /DA (/Helv 12 Tf 0 g) /P 3 0 R >>"
                .to_vec(),
        ];

        let mut bytes: Vec<u8> = b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n".to_vec();
        let mut offsets = Vec::new();
        for (index, body) in objects.iter().enumerate() {
            offsets.push(bytes.len());
            bytes.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
            bytes.extend_from_slice(body);
            bytes.extend_from_slice(b"\nendobj\n");
        }
        let xref_at = bytes.len();
        bytes.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
        bytes.extend_from_slice(b"0000000000 65535 f \n");
        for offset in &offsets {
            bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        bytes.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        bytes
    }

    #[test]
    fn a_filled_form_field_reaches_the_saved_file() {
        let _session = exclusive_session();
        // M5 records AcroForm fill as complete. What was never checked is
        // whether a value typed into a field survives a save: the field model
        // lives in the session, and the session is discarded when the document
        // closes. [FR-FORM-1, FR-FORM-4, PRIN-6]
        let _worker = stage_worker_beside_test_executable();

        let source = std::env::temp_dir().join("pdf-platform-bridge-form.pdf");
        std::fs::write(&source, acroform_pdf()).expect("write form fixture");

        open_document_impl(&source.to_string_lossy(), "").expect("open form document");

        let listed = list_form_fields_impl().expect("list fields");
        assert!(
            listed.contains("claimant"),
            "the document's own field was not imported: {listed}"
        );

        set_form_field_impl("claimant", "Ada Lovelace").expect("set field");

        let out = std::env::temp_dir().join("pdf-platform-bridge-form-out.pdf");
        let _ = std::fs::remove_file(&out);
        save_document_impl(&out.to_string_lossy()).expect("save");
        close_document_impl();

        let saved = std::fs::read(&out).expect("read saved");
        let text = String::from_utf8_lossy(&saved);
        assert!(
            text.contains("Ada Lovelace"),
            "the filled value is not in the saved file, so no reader will show it"
        );

        // And the document must still open, with the value visible to us again.
        open_document_impl(&out.to_string_lossy(), "").expect("reopen saved form");
        let relisted = list_form_fields_impl().expect("list after reopen");
        assert!(
            relisted.contains("Ada Lovelace"),
            "the value did not survive a save/reopen round trip: {relisted}"
        );
        close_document_impl();

        let _ = std::fs::remove_file(&source);
        let _ = std::fs::remove_file(&out);
        let _ = std::fs::remove_file(out.with_extension("xfdf"));
    }

    #[test]
    fn annotate_save_and_reopen_through_the_bridge() {
        let _session = exclusive_session();
        let fixture = fixture();
        assert!(fixture.is_file(), "fixture missing: {}", fixture.display());
        // No skip: a test that returns quietly when its subject is missing
        // cannot fail, which is the defect pattern this suite exists to catch.
        let _worker = stage_worker_beside_test_executable();

        let opened = open_document_impl(&fixture.to_string_lossy(), "")
            .expect("open through the bridge");
        assert_eq!(opened.page_count, 1);

        // Nothing yet.
        assert_eq!(annotation_count_impl().expect("count"), 0);

        let id = add_annotation_impl(0, "highlight", 72.0, 700.0, 120.0, 16.0, "on the record")
            .expect("add annotation");
        assert!(id > 0, "annotation id must be usable as a handle");
        assert_eq!(annotation_count_impl().expect("count"), 1);

        // XFDF must carry the annotation out. [FR-ANNOT-5]
        let xfdf = export_xfdf_impl().expect("export xfdf");
        assert!(xfdf.contains("on the record"), "xfdf lost the contents: {xfdf}");
        // XFDF names the element after the annotation type: `<Highlight ...>`.
        assert!(
            xfdf.contains("<Highlight "),
            "xfdf lost the annotation type: {xfdf}"
        );

        // Undo must remove it, redo must bring it back. [FR-ANNOT-4]
        undo_impl().expect("undo");
        assert_eq!(annotation_count_impl().expect("count after undo"), 0);
        redo_impl().expect("redo");
        assert_eq!(annotation_count_impl().expect("count after redo"), 1);

        let out = std::env::temp_dir().join("pdf-platform-bridge-annot.pdf");
        let _ = std::fs::remove_file(&out);
        save_document_impl(&out.to_string_lossy()).expect("save");
        close_document_impl();

        // The claim under test: the annotation is *in the file*, referenced by
        // the page. A saved file that merely grew proves nothing — the stamp
        // path passed that bar for months while drawing nothing.
        let saved = std::fs::read(&out).expect("read saved file");
        let text = String::from_utf8_lossy(&saved);
        assert!(
            text.contains("/Annots"),
            "the saved page has no /Annots array, so no reader will show the annotation"
        );
        assert!(
            text.contains("/Subtype /Highlight") || text.contains("/Subtype/Highlight"),
            "no highlight annotation dictionary in the saved file"
        );
        assert!(
            text.contains("on the record"),
            "the annotation's contents did not survive the save"
        );

        // And it must still be a document: reopening exercises the parser over
        // our own output, which is where the object over-read used to bite.
        let reopened = open_document_impl(&out.to_string_lossy(), "").expect("reopen saved file");
        assert_eq!(reopened.page_count, 1, "the saved file lost its page");
        close_document_impl();

        let _ = std::fs::remove_file(&out);
        let _ = std::fs::remove_file(out.with_extension("xfdf"));
    }
}
