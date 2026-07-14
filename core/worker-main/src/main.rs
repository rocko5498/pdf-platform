//! Z1 worker binary. Never holds authoritative document state. [ADR-008, SDS §2.3]
//!
//! M1: IPC + document handle + shmem tile smoke + render_tile with engine backend.
//! Commands arrive as typed `Command` envelopes (or legacy raw bytes);
//! responses are typed `WorkerEvent` frames.

use std::fs::File;
use std::process::ExitCode;
use std::time::Duration;

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
                eprintln!("worker: loaded PDFium engine ({} pages)", engine.page_count());
                return Some(Box::new(engine));
            }
            Err(e) => {
                eprintln!("worker: PDFium load failed, falling back to stub: {e}");
            }
        }
    }
    None
}

fn scan_and_encode(file: &File) -> Result<StructuralSummary, String> {
    let ds = scan_file(file).map_err(|e| e.to_string())?;
    Ok(StructuralSummary {
        page_count: ds.page_count,
        has_acroform: ds.has_acroform,
        has_xfa: ds.has_xfa,
        has_js: ds.has_js,
        sig_count: ds.sig_count,
        leniency_count: ds.leniency.len() as u32,
        leniency_events: ds.leniency.iter().map(|e| e.to_string()).collect(),
    })
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
