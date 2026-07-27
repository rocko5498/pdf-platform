//! Cold-start and first-page budget benchmarks. [ADR-023, SDS §14 M0, MET-PERF-1/2]
//!
//! Measures:
//! - Cold start: spawn worker + inspect document (structural summary)
//! - First page: open document + render first tile via the full IPC+shmem pipeline
//!
//! These are the M0 baseline measurements. Budgets from PRD §14:
//!   MET-PERF-1 (cold start): ≤ 1.0 s median, ≤ 1.5 s p95
//!   MET-PERF-2 (first page): ≤ 300 ms median, ≤ 600 ms p95
//!
//! Run: `cargo bench -p benchmarks --bench startup`

use std::path::PathBuf;
use std::time::{Duration, Instant};

use criterion::{criterion_group, criterion_main, Criterion};
use protocol::commands::{encode_command, Command};
use protocol::events::decode_worker_event;
use protocol::handles::TILE_RGBA8_BYTES;
use sandbox::shmem::SharedRegion;
use sandbox::spawn::{spawn_worker_with_attachments, SpawnAttachments};

fn fixture_path(name: &str) -> PathBuf {
    // Navigate from target/release/deps/ up to repo root.
    let exe = std::env::current_exe().expect("current exe");
    let repo_root = exe.parent() // deps/
        .and_then(|d| d.parent()) // release/
        .and_then(|d| d.parent()) // target/
        .and_then(|d| d.parent()) // core/
        .and_then(|d| d.parent()) // repo root
        .expect("repo root");
    repo_root.join("tools/corpus-diff/fixtures").join(name)
}

fn worker_path() -> PathBuf {
    // Bench binaries live in target/debug/deps/, worker is in target/debug/.
    let exe = std::env::current_exe().expect("current exe");
    let deps = exe.parent().expect("exe parent");
    let debug_dir = deps.parent().expect("debug dir");
    let worker = debug_dir.join(format!("worker{}", std::env::consts::EXE_SUFFIX));
    assert!(worker.exists(), "worker binary not found at {}; build first: cargo build -p worker-main", worker.display());
    worker
}

/// Benchmark: cold start = spawn worker + send Inspect + receive Summary.
///
/// This measures the full overhead of worker process creation, sandbox
/// lockdown (advisory in M0), IPC channel setup, and document scan.
fn bench_cold_start(c: &mut Criterion) {
    let pdf = fixture_path("valid-1page.pdf");
    if !pdf.exists() {
        eprintln!("fixture not found, skipping cold_start bench");
        return;
    }

    let worker = worker_path();
    let mut group = c.benchmark_group("cold_start");
    group.sample_size(50);

    group.bench_function("spawn_inspect_1page", |b| {
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                let doc_file = std::fs::File::open(&pdf).unwrap();
                let region = SharedRegion::create(TILE_RGBA8_BYTES).unwrap();

                let start = Instant::now();
                let mut child = spawn_worker_with_attachments(
                    &worker,
                    &SpawnAttachments { doc: Some(&doc_file), shmem: Some(region.file()), output: None, password: None },
                    &[],
                )
                .unwrap();

                let cmd = Command::Inspect { correlation_id: 1 };
                child.transport.send(&encode_command(&cmd)).unwrap();
                let reply = child.transport.recv_timeout(Duration::from_secs(10)).unwrap();
                let elapsed = start.elapsed();

                let event = decode_worker_event(&reply).expect("decode summary");
                match event {
                    protocol::events::WorkerEvent::Summary { .. } => {}
                    other => panic!("expected Summary, got {other:?}"),
                }

                let _ = child.transport.send(b"CMD:QUIT\n");
                let _ = child.child.wait();
                total += elapsed;
            }
            total
        });
    });

    group.finish();
}

/// Benchmark: first page = spawn worker + render first tile via RenderTile command.
///
/// Measures the full pipeline: worker spawn → IPC → PDFium render → shmem write →
/// TILE_READY response. This is the MET-PERF-2 budget target.
fn bench_first_page(c: &mut Criterion) {
    let pdf = fixture_path("valid-1page.pdf");
    if !pdf.exists() {
        eprintln!("fixture not found, skipping first_page bench");
        return;
    }

    let worker = worker_path();
    let mut group = c.benchmark_group("first_page");
    group.sample_size(50);

    group.bench_function("spawn_render_tile_256x256", |b| {
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                let doc_file = std::fs::File::open(&pdf).unwrap();
                let region = SharedRegion::create(TILE_RGBA8_BYTES).unwrap();

                let start = Instant::now();
                let mut child = spawn_worker_with_attachments(
                    &worker,
                    &SpawnAttachments { doc: Some(&doc_file), shmem: Some(region.file()), output: None, password: None },
                    &[],
                )
                .unwrap();

                let cmd = Command::RenderTile {
                    correlation_id: 1,
                    page: 0, x: 0, y: 0, w: 256, h: 256,
                    scale: 1.0, generation: 1, slot_offset: 0, col: 0, row: 0,
                };
                child.transport.send(&encode_command(&cmd)).unwrap();
                let reply = child.transport.recv_timeout(Duration::from_secs(10)).unwrap();
                let elapsed = start.elapsed();

                let event = decode_worker_event(&reply).expect("decode tile_ready");
                match event {
                    protocol::events::WorkerEvent::TileReady { .. } => {}
                    other => panic!("expected TileReady, got {other:?}"),
                }

                let _ = child.transport.send(b"CMD:QUIT\n");
                let _ = child.child.wait();
                total += elapsed;
            }
            total
        });
    });

    group.finish();
}

/// Benchmark: open + inspect + first tile (combined cold-start-to-first-pixel).
///
/// End-to-end M0 metric: from zero to a visible tile.
fn bench_cold_start_to_first_pixel(c: &mut Criterion) {
    let pdf = fixture_path("valid-1page.pdf");
    if !pdf.exists() {
        eprintln!("fixture not found, skipping cold_start_to_pixel bench");
        return;
    }

    let worker = worker_path();
    let mut group = c.benchmark_group("cold_start_to_pixel");
    group.sample_size(30);

    group.bench_function("open_inspect_render", |b| {
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                let start = Instant::now();

                let doc_file = std::fs::File::open(&pdf).unwrap();
                let region = SharedRegion::create(TILE_RGBA8_BYTES).unwrap();
                let mut child = spawn_worker_with_attachments(
                    &worker,
                    &SpawnAttachments { doc: Some(&doc_file), shmem: Some(region.file()), output: None, password: None },
                    &[],
                )
                .unwrap();

                // Step 1: Inspect.
                let cmd = Command::Inspect { correlation_id: 1 };
                child.transport.send(&encode_command(&cmd)).unwrap();
                let _ = child.transport.recv_timeout(Duration::from_secs(10)).unwrap();

                // Step 2: Render first tile.
                let cmd = Command::RenderTile {
                    correlation_id: 2,
                    page: 0, x: 0, y: 0, w: 256, h: 256,
                    scale: 1.0, generation: 1, slot_offset: 0, col: 0, row: 0,
                };
                child.transport.send(&encode_command(&cmd)).unwrap();
                let reply = child.transport.recv_timeout(Duration::from_secs(10)).unwrap();
                let elapsed = start.elapsed();

                let event = decode_worker_event(&reply).expect("decode tile_ready");
                match event {
                    protocol::events::WorkerEvent::TileReady { .. } => {}
                    other => panic!("expected TileReady, got {other:?}"),
                }

                let _ = child.transport.send(b"CMD:QUIT\n");
                let _ = child.child.wait();
                total += elapsed;
            }
            total
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_cold_start,
    bench_first_page,
    bench_cold_start_to_first_pixel,
);
criterion_main!(benches);
