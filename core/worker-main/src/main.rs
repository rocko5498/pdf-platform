//! Z1 worker binary. Never holds authoritative document state. [ADR-008, SDS §2.3]
//!
//! M1: IPC + document handle + shmem tile smoke + render_tile with engine backend.
//! Commands arrive as typed `Command` envelopes (or legacy raw bytes);
//! responses are typed `WorkerEvent` frames.

use std::fs::File;
use std::process::ExitCode;
use std::time::Duration;

use engine_api::extract::Extract;
use engine_api::rasterize::{Rasterize, RasterizeRequest, TileRect};
use pdf_cos::scan::scan_file;
use protocol::commands::{decode_command, Command};
use protocol::events::{encode_worker_event, WorkerEvent};
use protocol::handles::{encode_tile_ready, PixelFormat, TileSlotDesc, SHMEM_SMOKE_MAGIC, TILE_RGBA8_BYTES};
use protocol::inspect::StructuralSummary;
use protocol::transport::TransportError;
use sandbox::shmem::map_shmem_file;
use sandbox::spawn::{adopt_document_file, adopt_inherited, adopt_password, adopt_shmem_file};

fn main() -> ExitCode {
    // Apply sandbox confinement BEFORE any handle adoption or untrusted input.
    // SECURITY: human-gated — do not weaken filters. [ADR-016, IG AI-6, SDS §3.1]
    if let Err(e) = sandbox::confinement::lockdown_worker() {
        eprintln!("worker: confinement failed: {e}");
        return ExitCode::from(1);
    }

    let mut transport = match adopt_inherited() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("worker: adopt IPC failed: {e}");
            return ExitCode::from(1);
        }
    };

    let doc_file: Option<File> = match adopt_document_file() {
        Ok(f) => f,
        Err(e) => {
            eprintln!("worker: adopt document failed: {e}");
            return ExitCode::from(6);
        }
    };

    let shmem_file: Option<File> = match adopt_shmem_file() {
        Ok(f) => f,
        Err(e) => {
            eprintln!("worker: adopt shmem failed: {e}");
            return ExitCode::from(7);
        }
    };

    // Read password for encrypted documents (passed as env var by coordinator).
    let password: Option<String> = adopt_password();

    // Load the rendering engine from the inherited document handle.
    // Falls back to stub if PDFium is unavailable or no document is attached.
    let engine: Option<Box<dyn Rasterize>> = doc_file.as_ref().and_then(|f| {
        create_engine(f, password.as_deref())
    });

    // Also get a Structure reference for outline/layers/attachments queries.
    // SAFETY: PdfiumEngine is the only concrete type and implements both traits.
    let structure_engine: Option<Box<dyn engine_api::structure::Structure>> = doc_file.as_ref().and_then(|f| {
        create_structure_engine(f, password.as_deref())
    });

    // Extract engine for text extraction. [ADR-019, M2]
    let extract_engine: Option<Box<dyn Extract>> = doc_file.as_ref().and_then(|f| {
        create_extract_engine(f, password.as_deref())
    });

    loop {
        match transport.recv_timeout(Duration::from_secs(1)) {
            Ok(msg) => {
                // Try typed command decode first, fall through to legacy.
                match decode_command(&msg) {
                    Ok(cmd) => match cmd {
                        Command::Quit => break,
                        Command::Inspect { correlation_id } => {
                            handle_inspect(&doc_file, correlation_id, &mut transport);
                        }
                        Command::RenderTile {
                            correlation_id,
                            page,
                            x,
                            y,
                            w,
                            h,
                            scale,
                            generation,
                            slot_offset,
                            col,
                            row,
                        } => {
                            handle_render_tile_typed(
                                &engine,
                                &shmem_file,
                                correlation_id,
                                page,
                                x,
                                y,
                                w,
                                h,
                                scale,
                                generation,
                                slot_offset,
                                col,
                                row,
                                &mut transport,
                            );
                        }
                        Command::ExtractPage { correlation_id, page_index } => {
                            handle_extract_page(&extract_engine, correlation_id, page_index, &mut transport);
                        }
                        Command::GetOutline { correlation_id } => {
                            handle_get_outline(&structure_engine, correlation_id, &mut transport);
                        }
                        Command::GetLayers { correlation_id } => {
                            handle_get_layers(&structure_engine, correlation_id, &mut transport);
                        }
                        Command::GetAttachments { correlation_id } => {
                            handle_get_attachments(&structure_engine, correlation_id, &mut transport);
                        }
                        Command::GetObject { correlation_id, obj_num } => {
                            handle_get_object(&doc_file, correlation_id, obj_num, &mut transport);
                        }
                        // Coordinator-level commands — should not reach the worker.
                        Command::DeletePages { correlation_id, .. } |
                        Command::RotatePages { correlation_id, .. } |
                        Command::AddAnnotation { correlation_id, .. } |
                        Command::DeleteAnnotation { correlation_id, .. } => {
                            send_error(&mut transport, correlation_id, "organize/annotation commands are coordinator-level");
                        }
                    },
                    Err(_) => {
                        // Legacy raw-byte fallback for M0 backward compatibility.
                        handle_legacy(&msg, &doc_file, &shmem_file, &mut transport);
                    }
                }
            }
            Err(TransportError::Timeout) => continue,
            Err(TransportError::Disconnected) => break,
            Err(e) => {
                eprintln!("worker: recv failed: {e}");
                return ExitCode::from(3);
            }
        }
    }
    ExitCode::SUCCESS
}

// ---------------------------------------------------------------------------
// Typed command handlers
// ---------------------------------------------------------------------------

fn handle_inspect(doc_file: &Option<File>, correlation_id: u64, transport: &mut Box<dyn protocol::transport::WorkerTransport>) {
    let Some(file) = doc_file.as_ref() else {
        eprintln!("worker: inspect requested but no inherited document");
        send_error(transport, correlation_id, "no inherited document");
        return;
    };
    match scan_and_encode(file) {
        Ok(summary) => {
            let event = WorkerEvent::Summary { correlation_id, summary };
            let body = encode_worker_event(&event);
            if let Err(e) = transport.send(&body) {
                eprintln!("worker: send summary failed: {e}");
            }
        }
        Err(e) => {
            eprintln!("worker: inspect failed: {e}");
            send_error(transport, correlation_id, &e);
        }
    }
}

fn handle_render_tile_typed(
    engine: &Option<Box<dyn Rasterize>>,
    shmem_file: &Option<File>,
    correlation_id: u64,
    page: u32,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    scale: f32,
    generation: u64,
    slot_offset: u32,
    col: u32,
    row: u32,
    transport: &mut Box<dyn protocol::transport::WorkerTransport>,
) {
    let Some(file) = shmem_file.as_ref() else {
        eprintln!("worker: render_tile requested but no inherited shmem");
        send_error(transport, correlation_id, "no inherited shmem");
        return;
    };

    // Use the loaded engine, or fall back to stub for smoke tests.
    let output = match engine {
        Some(eng) => eng.rasterize(&RasterizeRequest {
            page_index: page,
            rect: TileRect { x, y, w, h },
            scale,
        }),
        None => {
            // No document attached — use stub for smoke/compatibility tests.
            let stub = engine_stub::StubEngine::new(1024);
            stub.rasterize(&RasterizeRequest {
                page_index: page,
                rect: TileRect { x, y, w, h },
                scale,
            })
        }
    };

    match output {
        Ok(output) => {
            // Write pixels into shmem.
            let total_needed = slot_offset as usize + output.rgba_pixels.len();
            match map_shmem_file(file, total_needed) {
                Ok(mut map) => {
                    let slot = &mut map[slot_offset as usize..slot_offset as usize + output.rgba_pixels.len()];
                    slot.copy_from_slice(&output.rgba_pixels);
                    let _ = map.flush();

                    let desc = TileSlotDesc {
                        offset: slot_offset,
                        len: output.rgba_pixels.len() as u32,
                        format: PixelFormat::Rgba8,
                        generation,
                        page,
                        col,
                        row,
                    };
                    let event = WorkerEvent::TileReady { correlation_id, desc };
                    let body = encode_worker_event(&event);
                    if let Err(e) = transport.send(&body) {
                        eprintln!("worker: send TILE_READY failed: {e}");
                    }
                }
                Err(e) => {
                    eprintln!("worker: shmem map failed: {e}");
                    send_error(transport, correlation_id, &e.to_string());
                }
            }
        }
        Err(e) => {
            eprintln!("worker: render_tile failed: {e}");
            send_error(transport, correlation_id, &e.to_string());
        }
    }
}

fn handle_extract_page(
    extract_engine: &Option<Box<dyn engine_api::extract::Extract>>,
    correlation_id: u64,
    page_index: u32,
    transport: &mut Box<dyn protocol::transport::WorkerTransport>,
) {
    let Some(engine) = extract_engine.as_ref() else {
        send_error(transport, correlation_id, "no extract engine loaded");
        return;
    };

    match engine.extract_page(page_index) {
        Ok(model) => {
            let line_geom: Vec<String> = model
                .lines
                .iter()
                .map(|l| {
                    let text = l
                        .text
                        .replace('\\', "\\\\")
                        .replace('|', "\\p")
                        .replace('\n', "\\n");
                    format!(
                        "{}|{}|{}|{}|{}|{}",
                        l.index, l.x, l.y, l.width, l.height, text
                    )
                })
                .collect();
            let event = WorkerEvent::TextExtracted {
                correlation_id,
                page_index,
                line_count: model.lines.len() as u32,
                char_count: model.char_count,
                reliable: model.reliable,
                has_structure: model.has_structure,
                full_text: model.full_text(),
                line_geom,
            };
            let body = encode_worker_event(&event);
            if let Err(e) = transport.send(&body) {
                eprintln!("worker: send TEXT_EXTRACTED failed: {e}");
            }
        }
        Err(e) => {
            send_error(transport, correlation_id, &format!("extract failed: {e}"));
        }
    }
}

fn handle_get_outline(
    structure_engine: &Option<Box<dyn engine_api::structure::Structure>>,
    correlation_id: u64,
    transport: &mut Box<dyn protocol::transport::WorkerTransport>,
) {
    let Some(structure) = structure_engine.as_ref() else {
        send_error(transport, correlation_id, "no engine loaded");
        return;
    };
    match structure.outline() {
        Ok(outline) => {
            let total = outline.total_count() as u32;
            let data = format!("entries={}", outline.entries.len());
            let event = WorkerEvent::OutlineResult {
                correlation_id,
                entry_count: outline.entries.len() as u32,
                total_count: total,
                data,
            };
            let body = encode_worker_event(&event);
            if let Err(e) = transport.send(&body) {
                eprintln!("worker: send OUTLINE_RESULT failed: {e}");
            }
        }
        Err(e) => {
            send_error(transport, correlation_id, &e.to_string());
        }
    }
}

fn handle_get_layers(
    structure_engine: &Option<Box<dyn engine_api::structure::Structure>>,
    correlation_id: u64,
    transport: &mut Box<dyn protocol::transport::WorkerTransport>,
) {
    let Some(structure) = structure_engine.as_ref() else {
        send_error(transport, correlation_id, "no engine loaded");
        return;
    };
    match structure.layers() {
        Ok(layers) => {
            let total = layers.total_count() as u32;
            let event = WorkerEvent::LayersResult {
                correlation_id,
                group_count: layers.groups.len() as u32,
                total_count: total,
                has_layers: !layers.is_empty(),
            };
            let body = encode_worker_event(&event);
            if let Err(e) = transport.send(&body) {
                eprintln!("worker: send LAYERS_RESULT failed: {e}");
            }
        }
        Err(e) => {
            send_error(transport, correlation_id, &e.to_string());
        }
    }
}

fn handle_get_attachments(
    structure_engine: &Option<Box<dyn engine_api::structure::Structure>>,
    correlation_id: u64,
    transport: &mut Box<dyn protocol::transport::WorkerTransport>,
) {
    let Some(structure) = structure_engine.as_ref() else {
        send_error(transport, correlation_id, "no engine loaded");
        return;
    };
    match structure.attachments() {
        Ok(attachments) => {
            let count = attachments.files.len() as u32;
            let data = attachments.files.iter()
                .map(|a| format!("{} ({} bytes)", a.name, a.size))
                .collect::<Vec<_>>()
                .join(";");
            let event = WorkerEvent::AttachmentsResult {
                correlation_id,
                count,
                data,
            };
            let body = encode_worker_event(&event);
            if let Err(e) = transport.send(&body) {
                eprintln!("worker: send ATTACHMENTS_RESULT failed: {e}");
            }
        }
        Err(e) => {
            send_error(transport, correlation_id, &e.to_string());
        }
    }
}

fn handle_get_object(
    doc_file: &Option<File>,
    correlation_id: u64,
    obj_num: u32,
    transport: &mut Box<dyn protocol::transport::WorkerTransport>,
) {
    let Some(file) = doc_file.as_ref() else {
        send_error(transport, correlation_id, "no document loaded");
        return;
    };

    // Mmap the file and parse xref to find the object's byte offset.
    let map = match unsafe { memmap2::Mmap::map(file) } {
        Ok(m) => m,
        Err(e) => {
            send_error(transport, correlation_id, &format!("mmap failed: {e}"));
            return;
        }
    };

    // Find startxref offset (scan last 1024 bytes).
    let xref_offset = match find_startxref(&map) {
        Some(off) => off,
        None => {
            send_error(transport, correlation_id, "no startxref found");
            return;
        }
    };

    // Parse the xref table to get object offsets.
    let entries = match parse_xref_table(&map, xref_offset) {
        Some(e) => e,
        None => {
            send_error(transport, correlation_id, "failed to parse xref");
            return;
        }
    };

    let idx = obj_num as usize;
    if idx >= entries.len() || !entries[idx].in_use {
        send_error(transport, correlation_id, &format!("object {obj_num} not found"));
        return;
    }

    let offset = entries[idx].offset as usize;
    // Find endobj after the offset to determine object length.
    let obj_bytes = &map[offset..];
    let end = obj_bytes.windows(7).position(|w| w == b"endobj")
        .map(|p| offset + p + 7)
        .unwrap_or(offset + obj_bytes.len().min(4096));

    let data = map[offset..end].to_vec();

    let event = WorkerEvent::ObjectData {
        correlation_id,
        obj_num,
        data,
    };
    let body = encode_worker_event(&event);
    if let Err(e) = transport.send(&body) {
        eprintln!("worker: send OBJECT_DATA failed: {e}");
    }
}

/// Scan last 1024 bytes for `startxref\n<N>`, return N as a file offset.
fn find_startxref(data: &[u8]) -> Option<usize> {
    const NEEDLE: &[u8] = b"startxref";
    let search_start = data.len().saturating_sub(1024);
    let tail = &data[search_start..];
    let mut last = None;
    for i in 0..=tail.len().saturating_sub(NEEDLE.len()) {
        if &tail[i..i + NEEDLE.len()] == NEEDLE {
            last = Some(i);
        }
    }
    let pos = last?;
    let mut i = pos + NEEDLE.len();
    while i < tail.len() && matches!(tail[i], b' ' | b'\r' | b'\n') {
        i += 1;
    }
    let start = i;
    while i < tail.len() && tail[i].is_ascii_digit() {
        i += 1;
    }
    std::str::from_utf8(&tail[start..i]).ok()?.parse().ok()
}

#[derive(Clone, Default)]
struct XrefEntry {
    offset: u64,
    in_use: bool,
}

fn parse_xref_table(data: &[u8], offset: usize) -> Option<Vec<XrefEntry>> {
    let d = data.get(offset..)?;
    if !d.starts_with(b"xref") {
        return None;
    }
    let mut pos = 4;
    skip_ws(d, &mut pos);
    let mut entries: Vec<XrefEntry> = Vec::new();
    loop {
        if d.get(pos..).map_or(false, |s| s.starts_with(b"trailer")) {
            break;
        }
        let first = parse_uint(d, &mut pos)?;
        skip_ws(d, &mut pos);
        let count = parse_uint(d, &mut pos)?;
        skip_ws(d, &mut pos);
        // skip eol
        if d.get(pos) == Some(&b'\r') { pos += 1; }
        if d.get(pos) == Some(&b'\n') { pos += 1; }

        let needed = first + count;
        if entries.len() < needed {
            entries.resize(needed, XrefEntry::default());
        }
        for obj in first..first + count {
            if pos + 20 > d.len() { break; }
            let entry_bytes = &d[pos..pos + 20];
            let offset_bytes = &entry_bytes[0..10];
            let in_use = entry_bytes.get(17) == Some(&b'n');
            let byte_offset = std::str::from_utf8(offset_bytes)
                .ok()
                .and_then(|s| s.trim().parse::<u64>().ok())
                .unwrap_or(0);
            entries[obj] = XrefEntry { offset: byte_offset, in_use };
            pos += 20;
        }
    }
    Some(entries)
}

fn parse_uint(data: &[u8], pos: &mut usize) -> Option<usize> {
    let start = *pos;
    while *pos < data.len() && data[*pos].is_ascii_digit() {
        *pos += 1;
    }
    if *pos == start { return None; }
    std::str::from_utf8(&data[start..*pos]).ok()?.parse().ok()
}

fn skip_ws(data: &[u8], pos: &mut usize) {
    while *pos < data.len() && matches!(data[*pos], b' ' | b'\t' | b'\r' | b'\n') {
        *pos += 1;
    }
}

fn send_error(transport: &mut Box<dyn protocol::transport::WorkerTransport>, correlation_id: u64, message: &str) {
    let event = WorkerEvent::RenderError {
        correlation_id,
        message: message.to_string(),
    };
    let body = encode_worker_event(&event);
    if let Err(send_err) = transport.send(&body) {
        eprintln!("worker: send RENDER_ERROR failed: {send_err}");
    }
}

// ---------------------------------------------------------------------------
// Legacy handlers (M0 backward compatibility)
// ---------------------------------------------------------------------------

fn handle_legacy(
    msg: &[u8],
    _doc_file: &Option<File>,
    shmem_file: &Option<File>,
    transport: &mut Box<dyn protocol::transport::WorkerTransport>,
) {
    if msg == b"tile_smoke" {
        let Some(file) = shmem_file.as_ref() else {
            eprintln!("worker: tile_smoke requested but no inherited shmem");
            return;
        };
        match fill_tile_smoke(file) {
            Ok(body) => {
                if let Err(e) = transport.send(&body) {
                    eprintln!("worker: send TILE_READY failed: {e}");
                }
            }
            Err(e) => {
                eprintln!("worker: tile_smoke failed: {e}");
            }
        }
        return;
    }

    // Unknown legacy message — echo back (test support).
    if let Err(e) = transport.send(msg) {
        eprintln!("worker: echo failed: {e}");
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Create the best available rendering engine from an inherited document handle.
///
/// Prefers PDFium (real rendering) when the feature is enabled; falls back to stub.
/// `password` is used for encrypted documents.
fn create_engine(file: &File, password: Option<&str>) -> Option<Box<dyn Rasterize>> {
    #[cfg(feature = "pdfium")]
    {
        match engine_pdfium::PdfiumEngine::from_file_handle_with_password(file, password) {
            Ok(engine) => {
                eprintln!("worker: loaded PDFium engine ({} pages)", Rasterize::page_count(&engine));
                return Some(Box::new(engine));
            }
            Err(e) => {
                eprintln!("worker: PDFium load failed, falling back to stub: {e}");
            }
        }
    }
    None
}

/// Create a Structure engine for outline/layers/attachments queries.
#[cfg(feature = "pdfium")]
fn create_structure_engine(file: &File, password: Option<&str>) -> Option<Box<dyn engine_api::structure::Structure>> {
    match engine_pdfium::PdfiumEngine::from_file_handle_with_password(file, password) {
        Ok(engine) => Some(Box::new(engine)),
        Err(_) => None,
    }
}

/// Create an Extract engine for text extraction. [ADR-019, M2]
#[cfg(feature = "pdfium")]
fn create_extract_engine(file: &File, password: Option<&str>) -> Option<Box<dyn Extract>> {
    match engine_pdfium::PdfiumEngine::from_file_handle_with_password(file, password) {
        Ok(engine) => Some(Box::new(engine)),
        Err(_) => None,
    }
}

#[cfg(not(feature = "pdfium"))]
fn create_structure_engine(_file: &File, _password: Option<&str>) -> Option<Box<dyn engine_api::structure::Structure>> {
    None
}

#[cfg(not(feature = "pdfium"))]
fn create_extract_engine(_file: &File, _password: Option<&str>) -> Option<Box<dyn Extract>> {
    None
}

fn scan_and_encode(file: &File) -> Result<StructuralSummary, String> {
    let ds = scan_file(file).map_err(|e| e.to_string())?;
    // Try to get page dimensions from PDFium if available.
    let page_dimensions = get_page_dimensions(file);
    Ok(StructuralSummary {
        page_count: ds.page_count,
        has_acroform: ds.has_acroform,
        has_xfa: ds.has_xfa,
        has_js: ds.has_js,
        sig_count: ds.sig_count,
        leniency_count: ds.leniency.len() as u32,
        leniency_events: ds.leniency.iter().map(|e| e.to_string()).collect(),
        page_dimensions,
        original_offsets: ds.xref_offsets,
    })
}

/// Try to get page dimensions from PDFium.
/// Returns empty vec if PDFium is unavailable or the file can't be loaded.
fn get_page_dimensions(file: &File) -> Vec<(u32, u32, u32)> {
    use engine_api::structure::Structure;
    match engine_pdfium::PdfiumEngine::from_file_handle(file) {
        Ok(engine) => match engine.page_meta() {
            Ok(metas) => metas.into_iter().map(|m| (m.width.to_bits(), m.height.to_bits(), m.rotation)).collect(),
            Err(_) => Vec::new(),
        },
        Err(_) => Vec::new(),
    }
}

fn fill_tile_smoke(file: &File) -> Result<Vec<u8>, String> {
    let mut map = map_shmem_file(file, TILE_RGBA8_BYTES).map_err(|e| e.to_string())?;
    let buf = &mut map[..TILE_RGBA8_BYTES];
    buf[..SHMEM_SMOKE_MAGIC.len()].copy_from_slice(SHMEM_SMOKE_MAGIC);
    for b in &mut buf[SHMEM_SMOKE_MAGIC.len()..] {
        *b = 0xA5;
    }
    map.flush().map_err(|e| e.to_string())?;
    let desc = TileSlotDesc {
        offset: 0,
        len: TILE_RGBA8_BYTES as u32,
        format: PixelFormat::Rgba8,
        generation: 1,
        page: 0,
        col: 0,
        row: 0,
    };
    Ok(encode_tile_ready(&desc))
}
