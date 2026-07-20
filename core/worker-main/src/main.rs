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

        // ONE engine instance per document. Re-opening the same inherited handle for
    // Structure/Extract after Rasterize already loaded PDFium fails, which made
    // outline/layers report "no engine loaded" while tiles still rendered. [ADR-005]
    #[cfg(feature = "pdfium")]
    let pdfium: Option<engine_pdfium::PdfiumEngine> = doc_file.as_ref().and_then(|f| {
        match engine_pdfium::PdfiumEngine::from_file_handle_with_password(f, password.as_deref()) {
            Ok(engine) => {
                eprintln!(
                    "worker: loaded PDFium engine ({} pages)",
                    Rasterize::page_count(&engine)
                );
                Some(engine)
            }
            Err(e) => {
                eprintln!("worker: PDFium load failed (tiles may use stub): {e}");
                None
            }
        }
    });
    #[cfg(not(feature = "pdfium"))]
    let pdfium: Option<()> = None;

    loop {
        match transport.recv_timeout(Duration::from_secs(1)) {
            Ok(msg) => {
                // Try typed command decode first, fall through to legacy.
                match decode_command(&msg) {
                    Ok(cmd) => match cmd {
                        Command::Quit => break,
                        Command::Inspect { correlation_id } => {
                            handle_inspect(
                                &doc_file,
                                pdfium.as_ref(),
                                correlation_id,
                                &mut transport,
                            );
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
                                pdfium.as_ref().map(|e| e as &dyn Rasterize),
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
                            handle_extract_page(
                                pdfium.as_ref().map(|e| e as &dyn Extract),
                                correlation_id,
                                page_index,
                                &mut transport,
                            );
                        }
                        Command::GetOutline { correlation_id } => {
                            handle_get_outline(
                                pdfium.as_ref().map(|e| e as &dyn engine_api::structure::Structure),
                                correlation_id,
                                &mut transport,
                            );
                        }
                        Command::GetLayers { correlation_id } => {
                            handle_get_layers(
                                pdfium.as_ref().map(|e| e as &dyn engine_api::structure::Structure),
                                correlation_id,
                                &mut transport,
                            );
                        }
                        Command::GetAttachments { correlation_id } => {
                            handle_get_attachments(
                                pdfium.as_ref().map(|e| e as &dyn engine_api::structure::Structure),
                                correlation_id,
                                &mut transport,
                            );
                        }
                        Command::GetObject { correlation_id, obj_num } => {
                            handle_get_object(&doc_file, correlation_id, obj_num, &mut transport);
                        }
                        Command::RenderPageForOcr {
                            correlation_id,
                            page_index,
                            scale,
                        } => {
                            handle_render_page_for_ocr(
                                pdfium.as_ref().map(|e| e as &dyn Rasterize),
                                correlation_id,
                                page_index,
                                scale,
                                &mut transport,
                            );
                        }
                        Command::FormsCalc {
                            correlation_id,
                            expression,
                            fields,
                            enabled,
                        } => {
                            handle_forms_calc(
                                correlation_id,
                                &expression,
                                &fields,
                                enabled,
                                &mut transport,
                            );
                        }
                        // Coordinator-level commands — should not reach the worker.
                        Command::DeletePages { correlation_id, .. } |
                        Command::RotatePages { correlation_id, .. } |
                        Command::AddAnnotation { correlation_id, .. } |
                        Command::DeleteAnnotation { correlation_id, .. } |
                        Command::RedactByTerm { correlation_id, .. } => {
                            send_error(&mut transport, correlation_id, "organize/annotation/redaction commands are coordinator-level");
                        }
                        // M11 plugin commands — handled by coordinator, not document worker.
                        Command::LoadPlugin { correlation_id, .. } |
                        Command::UnloadPlugin { correlation_id, .. } |
                        Command::InvokePluginAction { correlation_id, .. } => {
                            send_error(&mut transport, correlation_id, "plugin commands are coordinator-level");
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

fn handle_inspect(
    doc_file: &Option<File>,
    #[cfg(feature = "pdfium")] pdfium: Option<&engine_pdfium::PdfiumEngine>,
    #[cfg(not(feature = "pdfium"))] _pdfium: Option<&()>,
    correlation_id: u64,
    transport: &mut Box<dyn protocol::transport::WorkerTransport>,
) {
    let Some(file) = doc_file.as_ref() else {
        eprintln!("worker: inspect requested but no inherited document");
        send_error(transport, correlation_id, "no inherited document");
        return;
    };
    match scan_and_encode(file, pdfium) {
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
    engine: Option<&dyn Rasterize>,
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
    extract_engine: Option<&dyn Extract>,
    correlation_id: u64,
    page_index: u32,
    transport: &mut Box<dyn protocol::transport::WorkerTransport>,
) {
    let Some(engine) = extract_engine else {
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

/// Render a full page as a raster for OCR processing. [FR-OCR, M9]
///
/// Renders the entire page at the specified DPI scale and returns the
/// RGBA8 pixel data directly (not via shared memory). The coordinator
/// uses this to run OCR on the raster.
fn handle_render_page_for_ocr(
    engine: Option<&dyn Rasterize>,
    correlation_id: u64,
    page_index: u32,
    scale: f32,
    transport: &mut Box<dyn protocol::transport::WorkerTransport>,
) {
    let Some(eng) = engine else {
        send_error(transport, correlation_id, "no rasterize engine loaded");
        return;
    };

    // Get page dimensions by rendering a 1x1 tile to discover the page size.
    let page_count = eng.page_count();
    if page_index >= page_count {
        send_error(transport, correlation_id, &format!(
            "page {} out of range (document has {} pages)", page_index, page_count
        ));
        return;
    }

    // Render the full page at the requested scale.
    // We render the entire page as one tile by using a large rect.
    let output = eng.rasterize(&RasterizeRequest {
        page_index,
        rect: TileRect { x: 0, y: 0, w: 8192, h: 8192 },
        scale,
    });

    match output {
        Ok(tile) => {
            // Encode pixels as base64 for wire transport.
            use base64::Engine;
            let pixels_b64 = base64::engine::general_purpose::STANDARD.encode(&tile.rgba_pixels);

            let event = WorkerEvent::PageRasterReady {
                correlation_id,
                page_index,
                width: tile.width,
                height: tile.height,
                pixels_b64,
            };
            let body = encode_worker_event(&event);
            if let Err(e) = transport.send(&body) {
                eprintln!("worker: send PAGE_RASTER_READY failed: {e}");
            }
        }
        Err(e) => {
            send_error(transport, correlation_id, &format!("render for OCR failed: {e}"));
        }
    }
}

fn handle_get_outline(
    structure_engine: Option<&dyn engine_api::structure::Structure>,
    correlation_id: u64,
    transport: &mut Box<dyn protocol::transport::WorkerTransport>,
) {
    let Some(structure) = structure_engine else {
        send_error(transport, correlation_id, "no engine loaded");
        return;
    };
    match structure.outline() {
        Ok(outline) => {
            let total = outline.total_count() as u32;
            // Serialize each entry as "page|y|title" lines for shell navigation.
            // Nesting indicated by leading pipe count: 0 pipes = top-level, 1 pipe = child, etc.
            // [FR-BOOK, M1 exit: bookmark navigation]
            let mut lines = Vec::new();
            fn serialize_entries(entries: &[engine_api::structure::OutlineEntry], depth: u32, lines: &mut Vec<String>) {
                for entry in entries {
                    let prefix = "|".repeat(depth as usize);
                    let title_escaped = entry.title.replace('|', "\\p").replace('\n', "\\n");
                    lines.push(format!("{}{}|{}|{}", prefix, entry.page, entry.y, title_escaped));
                    serialize_entries(&entry.children, depth + 1, lines);
                }
            }
            serialize_entries(&outline.entries, 0, &mut lines);
            let data = format!("entries={}\n{}", outline.entries.len(), lines.join("\n"));
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
    structure_engine: Option<&dyn engine_api::structure::Structure>,
    correlation_id: u64,
    transport: &mut Box<dyn protocol::transport::WorkerTransport>,
) {
    let Some(structure) = structure_engine else {
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
    structure_engine: Option<&dyn engine_api::structure::Structure>,
    correlation_id: u64,
    transport: &mut Box<dyn protocol::transport::WorkerTransport>,
) {
    let Some(structure) = structure_engine else {
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

/// Evaluate forms JS subset expression in Z1 (isolated path). [ADR-017, FR-JS-*, M5]
fn handle_forms_calc(
    correlation_id: u64,
    expression: &str,
    fields: &[(String, f64)],
    enabled: bool,
    transport: &mut Box<dyn protocol::transport::WorkerTransport>,
) {
    use pdf_model::forms_js::{evaluate_expression, FormsJsError};
    use std::collections::HashMap;

    let event = if !enabled {
        WorkerEvent::FormsCalcResult {
            correlation_id,
            ok: false,
            value: 0.0,
            message: "forms JavaScript kill switch active".into(),
        }
    } else {
        let map: HashMap<String, f64> = fields.iter().cloned().collect();
        match evaluate_expression(expression, &map) {
            Ok(value) => WorkerEvent::FormsCalcResult {
                correlation_id,
                ok: true,
                value,
                message: String::new(),
            },
            Err(FormsJsError::Unsupported(s)) => WorkerEvent::FormsCalcResult {
                correlation_id,
                ok: false,
                value: 0.0,
                message: format!("unsupported: {s}"),
            },
            Err(e) => WorkerEvent::FormsCalcResult {
                correlation_id,
                ok: false,
                value: 0.0,
                message: e.to_string(),
            },
        }
    };
    let body = encode_worker_event(&event);
    if let Err(e) = transport.send(&body) {
        eprintln!("worker: send FORMS_CALC_RESULT failed: {e}");
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

fn scan_and_encode(
    file: &File,
    #[cfg(feature = "pdfium")] pdfium: Option<&engine_pdfium::PdfiumEngine>,
    #[cfg(not(feature = "pdfium"))] _pdfium: Option<&()>,
) -> Result<StructuralSummary, String> {
    let ds = scan_file(file).map_err(|e| e.to_string())?;
    // Prefer page meta from the already-loaded engine — never re-open the handle. [ADR-005]
    let page_dimensions = page_dimensions_from_engine(pdfium);
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

#[cfg(feature = "pdfium")]
fn page_dimensions_from_engine(
    pdfium: Option<&engine_pdfium::PdfiumEngine>,
) -> Vec<(u32, u32, u32)> {
    use engine_api::structure::Structure;
    let Some(engine) = pdfium else {
        return Vec::new();
    };
    match engine.page_meta() {
        Ok(metas) => metas
            .into_iter()
            .map(|m| (m.width.to_bits(), m.height.to_bits(), m.rotation))
            .collect(),
        Err(_) => Vec::new(),
    }
}

#[cfg(not(feature = "pdfium"))]
fn page_dimensions_from_engine(_pdfium: Option<&()>) -> Vec<(u32, u32, u32)> {
    Vec::new()
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
