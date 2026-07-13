//! Z1 worker binary. Never holds authoritative document state. [ADR-008, SDS §2.3]
//!
//! M0: IPC + document handle + shmem tile smoke + render_tile with stub engine.

use std::fs::File;
use std::process::ExitCode;
use std::time::Duration;

use engine_api::rasterize::{Rasterize, RasterizeRequest, TileRect};
use pdf_cos::scan::scan_file;
use protocol::commands::{decode_render_tile, CommandDecodeError};
use protocol::handles::{
    encode_tile_ready, PixelFormat, TileSlotDesc, SHMEM_SMOKE_MAGIC, TILE_RGBA8_BYTES,
};
use protocol::inspect::{encode_summary, StructuralSummary};
use protocol::transport::TransportError;
use sandbox::shmem::map_shmem_file;
use sandbox::spawn::{adopt_document_file, adopt_inherited, adopt_shmem_file};

fn main() -> ExitCode {
    // Apply sandbox confinement BEFORE any handle adoption or untrusted input.
    // SECURITY: human-gated — do not weaken filters. [ADR-016, IG AI-6, SDS §3.1]
    // M0: advisory lockdown (logs what would be applied, does not kill on failure).
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

    loop {
        match transport.recv_timeout(Duration::from_secs(1)) {
            Ok(msg) if msg == b"quit" => break,
            Ok(msg) if msg == b"inspect" => {
                let Some(file) = doc_file.as_ref() else {
                    eprintln!("worker: inspect requested but no inherited document");
                    return ExitCode::from(4);
                };
                match scan_and_encode(file) {
                    Ok(body) => {
                        if let Err(e) = transport.send(&body) {
                            eprintln!("worker: send summary failed: {e}");
                            return ExitCode::from(2);
                        }
                    }
                    Err(e) => {
                        eprintln!("worker: inspect failed: {e}");
                        return ExitCode::from(5);
                    }
                }
            }
            Ok(msg) if msg == b"tile_smoke" => {
                let Some(file) = shmem_file.as_ref() else {
                    eprintln!("worker: tile_smoke requested but no inherited shmem");
                    return ExitCode::from(8);
                };
                match fill_tile_smoke(file) {
                    Ok(body) => {
                        if let Err(e) = transport.send(&body) {
                            eprintln!("worker: send TILE_READY failed: {e}");
                            return ExitCode::from(2);
                        }
                    }
                    Err(e) => {
                        eprintln!("worker: tile_smoke failed: {e}");
                        return ExitCode::from(9);
                    }
                }
            }
            Ok(msg) if msg.starts_with(b"render_tile") => {
                let Some(file) = shmem_file.as_ref() else {
                    eprintln!("worker: render_tile requested but no inherited shmem");
                    return ExitCode::from(8);
                };
                match handle_render_tile(file, &msg) {
                    Ok(body) => {
                        if let Err(e) = transport.send(&body) {
                            eprintln!("worker: send TILE_READY failed: {e}");
                            return ExitCode::from(2);
                        }
                    }
                    Err(e) => {
                        eprintln!("worker: render_tile failed: {e}");
                        let err_msg = format!("RENDER_ERROR\n{e}").into_bytes();
                        if let Err(send_err) = transport.send(&err_msg) {
                            eprintln!("worker: send RENDER_ERROR failed: {send_err}");
                        }
                        return ExitCode::from(10);
                    }
                }
            }
            Ok(msg) => {
                if let Err(e) = transport.send(&msg) {
                    eprintln!("worker: send failed: {e}");
                    return ExitCode::from(2);
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

fn scan_and_encode(file: &File) -> Result<Vec<u8>, String> {
    let ds = scan_file(file).map_err(|e| e.to_string())?;
    let summary = StructuralSummary {
        page_count: ds.page_count,
        has_acroform: ds.has_acroform,
        has_xfa: ds.has_xfa,
        has_js: ds.has_js,
        sig_count: ds.sig_count,
        leniency_count: ds.leniency.len() as u32,
        leniency_events: ds.leniency.iter().map(|e| e.to_string()).collect(),
    };
    Ok(encode_summary(&summary))
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

/// Handle a `render_tile` command: parse request, rasterize via stub engine,
/// write pixels into shmem, return TILE_READY. [ADR-007, SDS §6]
fn handle_render_tile(shmem_file: &File, raw: &[u8]) -> Result<Vec<u8>, String> {
    let req = decode_render_tile(raw).map_err(|e| match e {
        CommandDecodeError::InvalidUtf8 => "invalid utf-8 in render_tile".to_string(),
        CommandDecodeError::UnknownCommand => "unknown render_tile version".to_string(),
        CommandDecodeError::BadField(f) => format!("missing/invalid field: {f}"),
    })?;

    // M0: use stub engine. Swap for engine-pdfium when prebuilt is pinned. [ADR-005]
    let engine = engine_stub::StubEngine::new(1024); // generous page count for testing

    let output = engine
        .rasterize(&RasterizeRequest {
            page_index: req.page,
            rect: TileRect { x: req.x, y: req.y, w: req.w, h: req.h },
            scale: req.scale,
        })
        .map_err(|e| e.to_string())?;

    // Write pixels into the shmem slot at the requested offset.
    let total_needed = req.slot_offset as usize + output.rgba_pixels.len();
    let mut map = map_shmem_file(shmem_file, total_needed).map_err(|e| e.to_string())?;
    let slot = &mut map[req.slot_offset as usize..req.slot_offset as usize + output.rgba_pixels.len()];
    slot.copy_from_slice(&output.rgba_pixels);
    map.flush().map_err(|e| e.to_string())?;

    let desc = TileSlotDesc {
        offset: req.slot_offset,
        len: output.rgba_pixels.len() as u32,
        format: PixelFormat::Rgba8,
        generation: req.generation,
        page: req.page,
        col: 0, // M0: single-tile, grid position not yet tracked in request
        row: 0,
    };
    Ok(encode_tile_ready(&desc))
}
