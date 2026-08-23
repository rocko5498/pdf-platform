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
use protocol::handles::{
    encode_tile_ready, PixelFormat, TileSlotDesc, SHMEM_SMOKE_MAGIC, TILE_RGBA8_BYTES,
};
use protocol::inspect::StructuralSummary;
use protocol::transport::TransportError;
use protocol::utility_jobs::{
    decode_command as decode_utility_job, encode_event as encode_utility_event, UtilityJobCommand,
    UtilityJobEvent, UtilityJobInput,
};
use sandbox::shmem::map_shmem_file;
use sandbox::spawn::{
    adopt_document_file, adopt_inherited, adopt_output_file, adopt_password, adopt_shmem_file,
};

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

    let _output_file: Option<File> = match adopt_output_file() {
        Ok(file) => file,
        Err(error) => {
            eprintln!("worker: adopt output failed: {error}");
            return ExitCode::from(8);
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

    // Detected once at startup; degrades to EngineUnavailable per-job if absent. [ADR-018]
    let ocr_engine = ocr_bridge::TesseractEngine::new();

    loop {
        match transport.recv_timeout(Duration::from_secs(1)) {
            Ok(msg) => {
                if let Ok(job) = decode_utility_job(&msg) {
                    let event = if let Err(message) = validate_utility_inputs(&job, &shmem_file) {
                        UtilityJobEvent::Failed {
                            correlation_id: job.correlation_id,
                            job_id: job.job_id,
                            message,
                        }
                    } else if job.operation == "noop" {
                        UtilityJobEvent::Completed {
                            correlation_id: job.correlation_id,
                            job_id: job.job_id,
                            output: Vec::new(),
                        }
                    } else if job.operation == "ocr" {
                        #[cfg(feature = "pdfium")]
                        let rasterizer = pdfium.as_ref().map(|e| e as &dyn Rasterize);
                        #[cfg(not(feature = "pdfium"))]
                        let rasterizer: Option<&dyn Rasterize> = None;
                        match execute_ocr_job(&job, rasterizer, &ocr_engine) {
                            Ok(output) => UtilityJobEvent::Completed {
                                correlation_id: job.correlation_id,
                                job_id: job.job_id,
                                output,
                            },
                            Err(message) => UtilityJobEvent::Failed {
                                correlation_id: job.correlation_id,
                                job_id: job.job_id,
                                message,
                            },
                        }
                    } else if job.operation == "thumbnail" {
                        #[cfg(feature = "pdfium")]
                        {
                            match (shmem_file.as_ref(), pdfium.as_ref()) {
                                (Some(shmem), Some(engine)) => {
                                    match execute_thumbnail_job(&job, shmem, engine) {
                                        Ok(output) => UtilityJobEvent::Completed {
                                            correlation_id: job.correlation_id,
                                            job_id: job.job_id,
                                            output,
                                        },
                                        Err(message) => UtilityJobEvent::Failed {
                                            correlation_id: job.correlation_id,
                                            job_id: job.job_id,
                                            message,
                                        },
                                    }
                                }
                                _ => UtilityJobEvent::Failed {
                                    correlation_id: job.correlation_id,
                                    job_id: job.job_id,
                                    message: "thumbnail requires document engine and shared memory"
                                        .into(),
                                },
                            }
                        }
                        #[cfg(not(feature = "pdfium"))]
                        {
                            UtilityJobEvent::Failed {
                                correlation_id: job.correlation_id,
                                job_id: job.job_id,
                                message: "thumbnail engine unavailable in this build".into(),
                            }
                        }
                    } else {
                        UtilityJobEvent::Failed {
                            correlation_id: job.correlation_id,
                            job_id: job.job_id,
                            message: format!("unsupported utility operation: {}", job.operation),
                        }
                    };
                    if let Ok(frame) = encode_utility_event(&event) {
                        let _ = transport.send(&frame);
                    }
                    continue;
                }
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
                        Command::ExtractPage {
                            correlation_id,
                            page_index,
                        } => {
                            handle_extract_page(
                                pdfium.as_ref().map(|e| e as &dyn Extract),
                                correlation_id,
                                page_index,
                                &mut transport,
                            );
                        }
                        Command::GetOutline { correlation_id } => {
                            handle_get_outline(
                                pdfium
                                    .as_ref()
                                    .map(|e| e as &dyn engine_api::structure::Structure),
                                correlation_id,
                                &mut transport,
                            );
                        }
                        Command::GetLayers { correlation_id } => {
                            handle_get_layers(&doc_file, correlation_id, &mut transport);
                        }
                        Command::GetAttachments { correlation_id } => {
                            handle_get_attachments(
                                pdfium
                                    .as_ref()
                                    .map(|e| e as &dyn engine_api::structure::Structure),
                                correlation_id,
                                &mut transport,
                            );
                        }
                        Command::GetObject {
                            correlation_id,
                            obj_num,
                        } => {
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
                        Command::DeletePages { correlation_id, .. }
                        | Command::RotatePages { correlation_id, .. }
                        | Command::AddAnnotation { correlation_id, .. }
                        | Command::DeleteAnnotation { correlation_id, .. }
                        | Command::RedactByTerm { correlation_id, .. } => {
                            send_error(
                                &mut transport,
                                correlation_id,
                                "organize/annotation/redaction commands are coordinator-level",
                            );
                        }
                        // M11 plugin commands — handled by coordinator, not document worker.
                        Command::LoadPlugin { correlation_id, .. }
                        | Command::UnloadPlugin { correlation_id, .. }
                        | Command::InvokePluginAction { correlation_id, .. } => {
                            send_error(
                                &mut transport,
                                correlation_id,
                                "plugin commands are coordinator-level",
                            );
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

fn validate_utility_inputs(
    job: &UtilityJobCommand,
    shmem_file: &Option<File>,
) -> Result<(), String> {
    for input in &job.inputs {
        let UtilityJobInput::SharedMemory {
            grant_id,
            offset,
            length,
        } = input
        else {
            continue;
        };
        if *grant_id == [0; 16] {
            return Err("invalid zero utility grant".into());
        }
        let file = shmem_file
            .as_ref()
            .ok_or_else(|| "utility shared memory not attached".to_string())?;
        let region_len = file
            .metadata()
            .map_err(|error| format!("utility shared memory metadata failed: {error}"))?
            .len();
        let end = offset
            .checked_add(*length)
            .ok_or_else(|| "utility shared memory range overflow".to_string())?;
        if end > region_len {
            return Err(format!(
                "utility shared memory range out of bounds: {end} > {region_len}"
            ));
        }
    }
    Ok(())
}

/// Render one page's raster in-process. [FR-OCR-1]
///
/// Shared by `handle_render_page_for_ocr` (legacy IPC path — kept for
/// compatibility, but its base64-over-IPC transport breaks past ~4MB of
/// pixels; not used by the OCR job path below) and `execute_ocr_job`, which
/// renders and recognizes in the same process so no raster ever crosses IPC.
fn rasterize_full_page(
    engine: &dyn Rasterize,
    page_index: u32,
    scale: f32,
) -> Result<engine_api::rasterize::TileOutput, String> {
    let page_count = engine.page_count();
    if page_index >= page_count {
        return Err(format!(
            "page {page_index} out of range (document has {page_count} pages)"
        ));
    }
    engine
        .rasterize(&RasterizeRequest {
            page_index,
            rect: TileRect {
                x: 0,
                y: 0,
                w: 8192,
                h: 8192,
            },
            scale,
        })
        .map_err(|e| e.to_string())
}

/// Run one OCR job: self-render the page, then recognize. [FR-OCR-1, ADR-018]
///
/// No page raster crosses IPC in either direction — the worker renders via
/// its own loaded document engine and recognizes in the same process,
/// unlike the shared-memory-input design this replaced (which required the
/// coordinator to pre-render and hand over a raster) or the legacy
/// `RenderPageForOcr` IPC path (which breaks on realistic page sizes).
fn execute_ocr_job(
    job: &UtilityJobCommand,
    rasterizer: Option<&dyn Rasterize>,
    engine: &dyn ocr_bridge::OcrEngine,
) -> Result<Vec<u8>, String> {
    let [UtilityJobInput::Inline(request)] = job.inputs.as_slice() else {
        return Err("OCR requires exactly one inline request".into());
    };
    let decoded = ocr_bridge::decode_utility_request(request)
        .map_err(|error| format!("OCR request decode failed: {error:?}"))?;
    let Some(rasterizer) = rasterizer else {
        return Err("ocr requires a loaded document engine".into());
    };
    let dpi_scale = decoded.options.target_dpi as f32 / 72.0;
    let tile = rasterize_full_page(rasterizer, decoded.page_index, dpi_scale)?;
    ocr_bridge::execute_utility_ocr(engine, request, &tile.rgba_pixels, tile.width, tile.height)
        .map_err(|error| format!("OCR execution failed: {error:?}"))
}

fn execute_thumbnail_job(
    job: &UtilityJobCommand,
    shmem_file: &File,
    engine: &dyn Rasterize,
) -> Result<Vec<u8>, String> {
    let [UtilityJobInput::Inline(request), UtilityJobInput::SharedMemory {
        grant_id,
        offset,
        length,
    }] = job.inputs.as_slice()
    else {
        return Err("thumbnail requires request metadata and one shared-memory output".into());
    };
    if *grant_id == [0; 16] {
        return Err("invalid zero thumbnail output grant".into());
    }
    let region_len = usize::try_from(
        shmem_file
            .metadata()
            .map_err(|error| format!("thumbnail shared-memory metadata failed: {error}"))?
            .len(),
    )
    .map_err(|_| "thumbnail shared-memory region is too large".to_string())?;
    let mut map = map_shmem_file(shmem_file, region_len)
        .map_err(|error| format!("thumbnail shared-memory map failed: {error}"))?;
    let start = usize::try_from(*offset).map_err(|_| "thumbnail output offset is too large")?;
    let output_len =
        usize::try_from(*length).map_err(|_| "thumbnail output length is too large")?;
    let end = start
        .checked_add(output_len)
        .ok_or_else(|| "thumbnail output range overflow".to_string())?;
    let output = map
        .get_mut(start..end)
        .ok_or_else(|| "thumbnail output range is out of bounds".to_string())?;
    let result = render_pipeline::thumbnail::render_thumbnail(engine, request, output)
        .map_err(|error| format!("thumbnail render failed: {error:?}"))?;
    map.flush()
        .map_err(|error| format!("thumbnail shared-memory flush failed: {error}"))?;
    Ok(result)
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
            let event = WorkerEvent::Summary {
                correlation_id,
                summary,
            };
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
                    let slot = &mut map
                        [slot_offset as usize..slot_offset as usize + output.rgba_pixels.len()];
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
                    let event = WorkerEvent::TileReady {
                        correlation_id,
                        desc,
                    };
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

    match rasterize_full_page(eng, page_index, scale) {
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
            send_error(
                transport,
                correlation_id,
                &format!("render for OCR failed: {e}"),
            );
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
            fn serialize_entries(
                entries: &[engine_api::structure::OutlineEntry],
                depth: u32,
                lines: &mut Vec<String>,
            ) {
                for entry in entries {
                    let prefix = "|".repeat(depth as usize);
                    let title_escaped = entry.title.replace('|', "\\p").replace('\n', "\\n");
                    lines.push(format!(
                        "{}{}|{}|{}",
                        prefix, entry.page, entry.y, title_escaped
                    ));
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
    doc_file: &Option<File>,
    correlation_id: u64,
    transport: &mut Box<dyn protocol::transport::WorkerTransport>,
) {
    // Answered from the document's own catalog rather than from the engine.
    // `PdfiumEngine::layers` returns an empty list for every document —
    // pdfium-render exposes no optional-content API — so the shell's Layers
    // panel reported "none" whether or not the document had any, which is a
    // silent wrong answer rather than an honest "cannot tell". [GR-8, FR-VIEW-4]
    let Some(file) = doc_file.as_ref() else {
        send_error(transport, correlation_id, "no document loaded");
        return;
    };
    let map = match unsafe { memmap2::Mmap::map(file) } {
        Ok(map) => map,
        Err(error) => {
            send_error(transport, correlation_id, &format!("mmap failed: {error}"));
            return;
        }
    };

    let groups = pdf_cos::ocg::parse_optional_content_groups(&map);
    let data = groups
        .iter()
        .map(|group| format!("{}	{}", group.name, group.visible))
        .collect::<Vec<_>>()
        .join("
");

    let event = WorkerEvent::LayersResult {
        correlation_id,
        group_count: groups.len() as u32,
        total_count: groups.len() as u32,
        has_layers: !groups.is_empty(),
        data,
    };
    let body = encode_worker_event(&event);
    if let Err(e) = transport.send(&body) {
        eprintln!("worker: send LAYERS_RESULT failed: {e}");
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
            let data = attachments
                .files
                .iter()
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
    // The whole /Prev chain. This used to be a private copy of the parser
    // that read only the newest section, so an object left untouched by an
    // incremental update — the shape of every third-party signed, commented
    // or filled PDF — could not be fetched. [FR-VIEW-2, SDS §3.1]
    let mut leniency = Vec::new();
    let entries = match pdf_cos::scan::parse_xref_chain(&map, xref_offset, &mut leniency) {
        Ok(entries) => entries,
        Err(error) => {
            send_error(transport, correlation_id, &format!("failed to parse xref: {error:?}"));
            return;
        }
    };

    // One fetch for both kinds of object. `fetch_object_bytes` slices the file
    // for an ordinary object and decompresses the containing object stream for
    // a PDF 1.5+ compressed one, which is where a modern producer keeps most of
    // the document. The private copy of this logic here could only do the
    // former, and it terminated objects with a `windows(7)` scan for the
    // six-byte `endobj` — the match never fired, so every object ran to the
    // 4096-byte fallback or to end of file. [SDS §3.1, PRIN-1, GR-8]
    let Some(data) = pdf_cos::scan::fetch_object_bytes(&map, &entries, obj_num) else {
        send_error(
            transport,
            correlation_id,
            &format!("object {obj_num} not found"),
        );
        return;
    };

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

fn send_error(
    transport: &mut Box<dyn protocol::transport::WorkerTransport>,
    correlation_id: u64,
    message: &str,
) {
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

#[cfg(test)]
mod utility_ocr_tests {
    use super::*;
    use engine_api::rasterize::{RasterizeError, TileOutput};
    use ocr_bridge::{
        decode_utility_result, encode_utility_request, OcrEngine, OcrPageResult, OcrUtilityRequest,
        PreprocessOptions,
    };

    struct FixtureEngine;

    impl OcrEngine for FixtureEngine {
        fn recognize(
            &self,
            _raster: &[u8],
            width: u32,
            height: u32,
            _source_dpi: u32,
            page_index: u32,
            _options: &PreprocessOptions,
        ) -> OcrPageResult {
            OcrPageResult {
                page_index,
                raster_width: width,
                raster_height: height,
                blocks: Vec::new(),
                full_text: "worker fixture".into(),
                average_confidence: 0.3,
                had_existing_text: false,
                orientation_correction: 0.0,
                success: true,
                error: None,
            }
        }

        fn is_available(&self) -> bool {
            true
        }

        fn name(&self) -> &str {
            "fixture"
        }
    }

    struct FixtureRasterizer;

    impl Rasterize for FixtureRasterizer {
        fn rasterize(&self, _request: &RasterizeRequest) -> Result<TileOutput, RasterizeError> {
            Ok(TileOutput {
                rgba_pixels: vec![7; 2 * 2 * 4],
                width: 2,
                height: 2,
            })
        }

        fn page_count(&self) -> u32 {
            5
        }
    }

    #[test]
    fn ocr_job_self_renders_via_engine_then_recognizes() {
        let request = OcrUtilityRequest {
            page_index: 4,
            language: "eng".into(),
            options: PreprocessOptions::default(),
        };
        let job = UtilityJobCommand {
            correlation_id: 1,
            job_id: 2,
            operation: "ocr".into(),
            inputs: vec![UtilityJobInput::Inline(
                encode_utility_request(&request).unwrap(),
            )],
        };

        let output = execute_ocr_job(&job, Some(&FixtureRasterizer as &dyn Rasterize), &FixtureEngine)
            .unwrap();
        let result = decode_utility_result(&output).unwrap();
        assert_eq!(result.page_index, 4);
        assert_eq!(result.full_text, "worker fixture");
        assert_eq!(result.raster_width, 2);
        assert_eq!(result.raster_height, 2);
    }
}

#[cfg(test)]
mod utility_thumbnail_tests {
    use super::*;
    use engine_api::rasterize::{RasterizeError, RasterizeRequest, TileOutput};
    use protocol::utility_thumbnails::{
        decode_thumbnail_result, encode_thumbnail_request, ThumbnailRequest,
    };
    use sandbox::shmem::SharedRegion;

    struct FixtureRasterizer;

    impl Rasterize for FixtureRasterizer {
        fn rasterize(&self, request: &RasterizeRequest) -> Result<TileOutput, RasterizeError> {
            Ok(TileOutput {
                rgba_pixels: vec![11; (request.rect.w * request.rect.h * 4) as usize],
                width: request.rect.w,
                height: request.rect.h,
            })
        }

        fn page_count(&self) -> u32 {
            1
        }
    }

    #[test]
    fn worker_writes_thumbnail_only_to_declared_output_grant() {
        let region = SharedRegion::create(80).unwrap();
        let request = ThumbnailRequest {
            page: 0,
            width: 4,
            height: 4,
            scale: 0.25,
            generation: 2,
            revision: 5,
        };
        let job = UtilityJobCommand {
            correlation_id: 1,
            job_id: 2,
            operation: "thumbnail".into(),
            inputs: vec![
                UtilityJobInput::Inline(encode_thumbnail_request(&request).unwrap()),
                UtilityJobInput::SharedMemory {
                    grant_id: [2; 16],
                    offset: 8,
                    length: 64,
                },
            ],
        };

        let encoded = execute_thumbnail_job(&job, region.file(), &FixtureRasterizer).unwrap();
        let result = decode_thumbnail_result(&encoded).unwrap();
        assert!(result.is_current(2, 5));
        assert!(region.as_slice()[..8].iter().all(|byte| *byte == 0));
        assert!(region.as_slice()[8..72].iter().all(|byte| *byte == 11));
        assert!(region.as_slice()[72..].iter().all(|byte| *byte == 0));
    }
}
