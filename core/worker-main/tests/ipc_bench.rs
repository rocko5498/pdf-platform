//! Cross-process IPC render_tile benchmark. [ADR-023, SDS §14, MET-PERF]
//!
//! Measures the full round-trip: send RenderTileRequest → worker rasterizes
//! via stub engine → writes into shmem → returns TILE_READY.
//!
//! This is a single-iteration benchmark (not criterion) to establish a
//! baseline. For criterion-based cross-process benchmarks, see benchmarks/ crate.
//!
//! Run: `cargo test -p worker-main --test ipc_bench -- --nocapture`

use std::path::Path;
use std::time::{Duration, Instant};

use protocol::commands::{encode_command, Command};
use protocol::events::{decode_worker_event, WorkerEvent};
use protocol::handles::TILE_RGBA8_BYTES;
use sandbox::shmem::SharedRegion;
use sandbox::spawn::{spawn_worker_with_attachments, SpawnAttachments};

fn worker_path() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_worker"))
}

#[test]
fn ipc_render_tile_roundtrip_baseline() {
    const ITERATIONS: u32 = 100;
    let mut times = Vec::with_capacity(ITERATIONS as usize);

    for i in 0..ITERATIONS {
        let region = SharedRegion::create(TILE_RGBA8_BYTES).expect("create shmem");
        let mut child = spawn_worker_with_attachments(
            worker_path(),
            &SpawnAttachments { doc: None, shmem: Some(region.file()), password: None },
            &[],
        )
        .expect("spawn");

        let cmd = Command::RenderTile {
            correlation_id: i as u64 + 1,
            page: 0, x: 0, y: 0, w: 256, h: 256,
            scale: 1.0, generation: 1, slot_offset: 0,
            col: 0, row: 0,
        };

        let start = Instant::now();
        child.transport.send(&encode_command(&cmd)).expect("send");
        let reply = child
            .transport
            .recv_timeout(Duration::from_secs(5))
            .expect("recv");
        let elapsed = start.elapsed();

        match decode_worker_event(&reply) {
            Ok(WorkerEvent::TileReady { desc, .. }) => {
                assert_eq!(desc.generation, 1);
            }
            other => panic!("expected TileReady, got {other:?}"),
        }
        times.push(elapsed);

        child.transport.send(b"CMD:QUIT\n").expect("quit");
        let _ = child.child.wait();
    }

    // Sort for percentiles.
    times.sort();

    let total: Duration = times.iter().sum();
    let mean = total / ITERATIONS;
    let median = times[ITERATIONS as usize / 2];
    let p95 = times[(ITERATIONS as f64 * 0.95) as usize];
    let p99 = times[(ITERATIONS as f64 * 0.99) as usize];
    let min = times[0];
    let max = times[ITERATIONS as usize - 1];

    println!();
    println!("═══ IPC render_tile roundtrip ({ITERATIONS} iterations) ═══");
    println!("  mean:  {mean:?}");
    println!("  median: {median:?}");
    println!("  p95:   {p95:?}");
    println!("  p99:   {p99:?}");
    println!("  min:   {min:?}");
    println!("  max:   {max:?}");
    println!("═══════════════════════════════════════════════════════");
    println!();
}
