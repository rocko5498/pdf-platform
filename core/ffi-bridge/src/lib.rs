//! Sole Rust↔Qt FFI boundary. [ADR-004]
//!
//! RULES (enforced in review):
//!   FFI-1: cxx-checked interface only — no hand-rolled ABI.
//!   FFI-3: no raw pointers owned across the boundary.
//!   FFI-4: carries commands/events/handles only — never document objects.
//!   FFI-6: two-reviewer rule; changes require one FFI-surface owner. [ADR-027]
// SAFETY: cxx guarantees type-checked cross-language calls; no exceptions cross
//         this boundary; ownership does not straddle languages. [ADR-004, ADR-027]

use std::os::windows::io::AsRawHandle;

use protocol::commands::{encode_render_tile, RenderTileRequest};
use protocol::handles::{decode_tile_ready, TILE_RGBA8_BYTES};
use protocol::transport::WorkerTransport;
use sandbox::shmem::SharedRegion;
use sandbox::spawn::{spawn_worker_with_attachments, SpawnAttachments};

/// Holds the state for one open document's worker connection.
struct DocSession {
    child: sandbox::spawn::WorkerChild,
    #[allow(dead_code)]
    region: SharedRegion,
}

/// Global session storage (M0: one document at a time).
static mut SESSION: Option<Box<DocSession>> = None;

/// Open a document: spawn worker, create shmem, store session.
fn open_document_impl(_path: &str) -> Result<ffi::OpenResultFFI, String> {
    let region = SharedRegion::create(TILE_RGBA8_BYTES).map_err(|e| e.to_string())?;

    let exe = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .parent()
        .ok_or("no exe parent")?
        .join("worker.exe");

    let child = spawn_worker_with_attachments(
        &exe,
        &SpawnAttachments { doc: None, shmem: Some(region.file()) },
        &[],
    )
    .map_err(|e| e.to_string())?;

    // SAFETY: raw_handle() returns HANDLE (void*) on Windows, cast to isize for FFI.
    let handle = region.file().as_raw_handle() as isize;
    let page_count = 1024u32; // stub engine

    // SAFETY: single-threaded Qt main thread; M0 single-document.
    unsafe {
        SESSION = Some(Box::new(DocSession { child, region }));
    }

    Ok(ffi::OpenResultFFI { page_count, shmem_handle: handle })
}

/// Render a tile. Returns (offset, len, generation).
fn render_tile_impl(
    page: u32,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    scale: f32,
    generation: u64,
) -> Result<ffi::TileResultFFI, String> {
    // SAFETY: single-threaded Qt main thread.
    let session = unsafe { SESSION.as_mut().ok_or("no open document")? };

    let req = RenderTileRequest { page, x, y, w, h, scale, generation, slot_offset: 0 };
    let body = encode_render_tile(&req);
    session.child.transport.send(&body).map_err(|e| e.to_string())?;
    let reply = session.child
        .transport
        .recv_timeout(std::time::Duration::from_secs(5))
        .map_err(|e| e.to_string())?;
    let desc = decode_tile_ready(&reply).map_err(|e| e.to_string())?;
    Ok(ffi::TileResultFFI { offset: desc.offset, len: desc.len, generation: desc.generation })
}

/// Close the current document session.
fn close_document_impl() {
    // SAFETY: single-threaded Qt main thread.
    if let Some(mut session) = unsafe { SESSION.take() } {
        let _ = session.child.transport.send(b"quit");
        let _ = session.child.child.wait();
    }
}

#[cxx::bridge(namespace = "pdf_platform")]
mod ffi {
    /// Result of opening a document.
    struct OpenResultFFI {
        page_count: u32,
        /// Windows HANDLE for the shared memory region.
        shmem_handle: isize,
    }

    /// Tile descriptor returned after rendering.
    struct TileResultFFI {
        offset: u32,
        len: u32,
        generation: u64,
    }

    extern "Rust" {
        /// Open a document and spawn a worker. Returns page count and shmem handle.
        fn open_document_impl(path: &str) -> Result<OpenResultFFI>;

        /// Render a tile. Must call after open_document.
        fn render_tile_impl(
            page: u32,
            x: u32,
            y: u32,
            w: u32,
            h: u32,
            scale: f32,
            generation: u64,
        ) -> Result<TileResultFFI>;

        /// Close the document and kill the worker.
        fn close_document_impl();
    }
}
