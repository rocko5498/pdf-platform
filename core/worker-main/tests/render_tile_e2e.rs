//! End-to-end render_tile: coordinator sends request, worker rasterizes via stub engine,
//! pixels appear in shmem, coordinator verifies. [ADR-007, SDS §6, M0]
//!
//! Proves the full render pipeline works without PDFium.

use std::path::Path;
use std::time::Duration;

use protocol::commands::encode_render_tile;
use protocol::handles::{decode_tile_ready, PixelFormat, TILE_RGBA8_BYTES};
use protocol::transport::WorkerTransport as _;
use sandbox::shmem::SharedRegion;
use sandbox::spawn::{spawn_worker_with_attachments, SpawnAttachments};

fn worker_path() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_worker"))
}

#[test]
fn render_tile_stub_engine_produces_colored_pixels() {
    // 1. Create a shmem region large enough for one tile.
    let region = SharedRegion::create(TILE_RGBA8_BYTES).expect("create shmem");

    // 2. Spawn worker with shmem attached.
    let mut child = spawn_worker_with_attachments(
        worker_path(),
        &SpawnAttachments { doc: None, shmem: Some(region.file()) },
        &[],
    )
    .expect("spawn with shmem");

    // 3. Send render_tile command: page 0, full 256x256 tile, scale 1.0.
    let req = encode_render_tile(&protocol::commands::RenderTileRequest {
        page: 0,
        x: 0,
        y: 0,
        w: 256,
        h: 256,
        scale: 1.0,
        generation: 1,
        slot_offset: 0,
    });
    child.transport.send(&req).expect("send render_tile");

    // 4. Receive TILE_READY.
    let reply = child
        .transport
        .recv_timeout(Duration::from_secs(5))
        .expect("recv TILE_READY");
    let desc = decode_tile_ready(&reply).expect("decode TILE_READY");

    // 5. Verify descriptor.
    assert_eq!(desc.offset, 0);
    assert_eq!(desc.len as usize, TILE_RGBA8_BYTES);
    assert_eq!(desc.format, PixelFormat::Rgba8);
    assert_eq!(desc.generation, 1);
    assert_eq!(desc.page, 0, "tile identity: page should be 0");
    assert_eq!(desc.col, 0, "tile identity: col should be 0");
    assert_eq!(desc.row, 0, "tile identity: row should be 0");

    // 6. Read pixels from shmem and verify they are real (non-zero, colored).
    let _ = region.flush();
    let bytes = region.as_slice();
    assert_eq!(bytes.len(), TILE_RGBA8_BYTES);

    // First pixel: R should be non-zero (stub engine page 0 = red hue).
    assert_ne!(bytes[0], 0, "red channel should be non-zero");
    // Alpha should be 255.
    assert_eq!(bytes[3], 255, "alpha should be 255");

    // Verify checkerboard pattern: pixel at (0,0) and pixel at (32,0) differ.
    let pixel0_r = bytes[0];
    let pixel32_r = bytes[32 * 4]; // 32 pixels to the right
    assert_ne!(pixel0_r, pixel32_r, "checkerboard should produce different brightness");

    // Save page 0 green channel for comparison.
    let page0_green = bytes[1];

    // Verify page 1 produces a different hue (yellow vs red).
    let req_p1 = encode_render_tile(&protocol::commands::RenderTileRequest {
        page: 1,
        x: 0,
        y: 0,
        w: 256,
        h: 256,
        scale: 1.0,
        generation: 2,
        slot_offset: 0,
    });
    child.transport.send(&req_p1).expect("send render_tile page 1");
    let reply_p1 = child
        .transport
        .recv_timeout(Duration::from_secs(5))
        .expect("recv TILE_READY page 1");
    let desc_p1 = decode_tile_ready(&reply_p1).expect("decode TILE_READY page 1");
    assert_eq!(desc_p1.generation, 2);

    let _ = region.flush();
    let bytes_p1 = region.as_slice();
    // Page 0 (red hue, G≈68) and page 1 (yellow hue, G≈229) differ in green channel.
    assert_ne!(page0_green, bytes_p1[1], "page 0 and page 1 should have different green channel");

    child.transport.send(b"quit").expect("quit");
    let _ = child.child.wait();
}

#[test]
fn render_tile_page_out_of_range_returns_error() {
    let region = SharedRegion::create(TILE_RGBA8_BYTES).expect("create shmem");

    let mut child = spawn_worker_with_attachments(
        worker_path(),
        &SpawnAttachments { doc: None, shmem: Some(region.file()) },
        &[],
    )
    .expect("spawn with shmem");

    // Request page 9999 — stub engine has 1024 pages, so this should work,
    // but let's test a truly out-of-range page.
    let req = encode_render_tile(&protocol::commands::RenderTileRequest {
        page: 99999,
        x: 0,
        y: 0,
        w: 64,
        h: 64,
        scale: 1.0,
        generation: 1,
        slot_offset: 0,
    });
    child.transport.send(&req).expect("send render_tile");

    let reply = child
        .transport
        .recv_timeout(Duration::from_secs(5))
        .expect("recv response");
    // Should get RENDER_ERROR, not TILE_READY.
    let text = std::str::from_utf8(&reply).expect("response should be utf-8");
    assert!(text.starts_with("RENDER_ERROR"), "expected RENDER_ERROR, got: {text}");

    child.transport.send(b"quit").expect("quit");
    let _ = child.child.wait();
}
