//! Z1 worker binary. Never holds authoritative document state. [ADR-008, SDS §2.3]
//!
//! M0: IPC + document handle + optional shmem tile smoke (no PDFium).

use std::fs::File;
use std::process::ExitCode;
use std::time::Duration;

use pdf_cos::scan::scan_file;
use protocol::handles::{
    encode_tile_ready, PixelFormat, TileSlotDesc, SHMEM_SMOKE_MAGIC, TILE_RGBA8_BYTES,
};
use protocol::inspect::{encode_summary, StructuralSummary};
use protocol::transport::{TransportError, WorkerTransport as _};
use sandbox::shmem::map_shmem_file;
use sandbox::spawn::{adopt_document_file, adopt_inherited, adopt_shmem_file};

fn main() -> ExitCode {
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
    };
    Ok(encode_tile_ready(&desc))
}
