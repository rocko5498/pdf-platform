//! Cross-process shmem tile smoke: worker fills, parent verifies. [SDS §4.2, §6.3]
//! Design: docs/superpowers/specs/2026-07-12-shmem-tile-design.md

use std::path::Path;
use std::time::Duration;

use protocol::handles::{
    decode_tile_ready, PixelFormat, SHMEM_SMOKE_MAGIC, TILE_RGBA8_BYTES,
};
use protocol::transport::WorkerTransport as _;
use sandbox::shmem::SharedRegion;
use sandbox::spawn::{spawn_worker_with_attachments, SpawnAttachments};

fn worker_path() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_worker"))
}

#[test]
fn worker_fills_shmem_tile_smoke() {
    let region = SharedRegion::create(TILE_RGBA8_BYTES).expect("create shmem");
    assert_eq!(region.len(), TILE_RGBA8_BYTES);

    let mut child = spawn_worker_with_attachments(
        worker_path(),
        &SpawnAttachments {
            doc: None,
            shmem: Some(region.file()),
        },
        &[],
    )
    .expect("spawn with shmem");

    child
        .transport
        .send(b"tile_smoke")
        .expect("send tile_smoke");
    let reply = child
        .transport
        .recv_timeout(Duration::from_secs(5))
        .expect("recv TILE_READY");
    let desc = decode_tile_ready(&reply).expect("decode TILE_READY");
    assert_eq!(desc.offset, 0);
    assert_eq!(desc.len as usize, TILE_RGBA8_BYTES);
    assert_eq!(desc.format, PixelFormat::Rgba8);
    assert_eq!(desc.generation, 1);

    // Ensure worker flush is visible (mapping is shared).
    let _ = region.flush();
    let bytes = region.as_slice();
    assert_eq!(&bytes[..SHMEM_SMOKE_MAGIC.len()], SHMEM_SMOKE_MAGIC);
    assert!(
        bytes[SHMEM_SMOKE_MAGIC.len()..].iter().all(|&b| b == 0xA5),
        "expected 0xA5 fill"
    );

    child.transport.send(b"quit").expect("quit");
    let _ = child.child.wait();
}
