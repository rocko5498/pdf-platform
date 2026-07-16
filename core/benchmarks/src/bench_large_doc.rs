//! Large-document scroll benchmarks. [ADR-023, SDS §14 M1, MET-PERF-3]
//!
//! Measures render pipeline performance on large (2,000-page) documents:
//! - Viewport decomposition + scheduling for a scroll through many pages
//! - Tile scheduling throughput (how fast can we produce tile requests)
//! - Prefetch margin computation under fast-scroll velocity
//!
//! These benchmarks validate the M1 exit criteria:
//!   MET-PERF-3: Smooth-scroll frame-time at p95 on large documents
//!
//! Run: `cargo bench -p benchmarks --bench large_doc`

use std::time::{Duration, Instant};

use criterion::{criterion_group, criterion_main, Criterion};
use protocol::commands::{encode_command, Command};
use protocol::events::decode_worker_event;
use protocol::handles::TILE_RGBA8_BYTES;
use render_pipeline::layout::{PageGeometry, PagePositioner, ViewportState};

use sandbox::shmem::SharedRegion;
use sandbox::spawn::{spawn_worker_with_attachments, SpawnAttachments};

/// Generate a minimal N-page PDF and write it to a temp file.
fn generate_temp_pdf(num_pages: u32, label: &str) -> std::path::PathBuf {
    let mut buf = Vec::with_capacity(256 + num_pages as usize * 80);
    let mut offsets = Vec::with_capacity(num_pages as usize + 3);

    buf.extend_from_slice(b"%PDF-1.4\n");
    offsets.push(buf.len() as u32);
    buf.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");

    offsets.push(buf.len() as u32);
    let kids: Vec<String> = (0..num_pages).map(|i| format!("{} 0 R", i + 3)).collect();
    use std::io::Write;
    write!(&mut buf, "2 0 obj\n<< /Type /Pages /Kids [{}] /Count {} >>\nendobj\n", kids.join(" "), num_pages).unwrap();

    for i in 0..num_pages {
        offsets.push(buf.len() as u32);
        write!(&mut buf, "{} 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>\nendobj\n", i + 3).unwrap();
    }

    let xref_offset = buf.len() as u32;
    write!(&mut buf, "xref\n0 {}\n", num_pages + 3).unwrap();
    buf.extend_from_slice(b"0000000000 65535 f \n");
    for offset in &offsets {
        write!(&mut buf, "{:010} 00000 n \n", offset).unwrap();
    }
    write!(&mut buf, "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n", num_pages + 3, xref_offset).unwrap();

    let dir = std::env::temp_dir();
    let path = dir.join(format!("pdf-platform-bench-{}-{}p.pdf", label, num_pages));
    std::fs::write(&path, &buf).expect("write temp PDF");
    path
}

fn worker_path() -> std::path::PathBuf {
    let exe = std::env::current_exe().expect("current exe");
    let deps = exe.parent().expect("exe parent");
    let debug_dir = deps.parent().expect("debug dir");
    let worker = debug_dir.join(format!("worker{}", std::env::consts::EXE_SUFFIX));
    assert!(worker.exists(), "worker binary not found; build first: cargo build -p worker-main");
    worker
}

/// Benchmark: viewport decomposition + scheduling for a 2,000-page continuous scroll.
///
/// Measures how fast the render pipeline can:
/// 1. Position 2,000 pages in a continuous layout
/// 2. Decompose a viewport into tile requests
/// 3. Schedule tiles with deduplication
///
/// This is the core of scroll smoothness — if decomposition is fast,
/// the shell can publish viewports at frame cadence.
fn bench_viewport_scroll_2000_pages(c: &mut Criterion) {
    let mut group = c.benchmark_group("viewport_scroll_2000p");
    group.sample_size(50);

    // Set up a 2,000-page continuous layout.
    let geos: Vec<PageGeometry> = (0..2000)
        .map(|_| PageGeometry { width: 612.0, height: 792.0, rotation: 0 })
        .collect();
    let pos = PagePositioner::new(geos);

    // Simulate a viewport showing ~3 pages at 1.0x scale.
    let mut state = ViewportState::new(800.0, 600.0);
    state.layout = render_pipeline::layout::PageLayout::Continuous;
    state.scale = 1.0;

    group.bench_function("decompose_at_top", |b| {
        b.iter(|| {
            state.scroll_y = 0.0;
            pos.build_viewport(&state)
        });
    });

    group.bench_function("decompose_at_middle", |b| {
        b.iter(|| {
            state.scroll_y = 400_000.0; // ~500 pages down
            pos.build_viewport(&state)
        });
    });

    group.bench_function("decompose_at_bottom", |b| {
        b.iter(|| {
            state.scroll_y = 1_500_000.0; // near the end
            pos.build_viewport(&state)
        });
    });

    // Measure tile decomposition throughput.
    let viewport_at_middle = {
        state.scroll_y = 400_000.0;
        pos.build_viewport(&state)
    };

    group.bench_function("tile_requests_1viewport", |b| {
        b.iter(|| viewport_at_middle.decompose(1));
    });

    group.bench_function("tile_requests_with_prefetch", |b| {
        b.iter(|| viewport_at_middle.decompose_with_prefetch(1, 2));
    });

    group.finish();
}

/// Benchmark: full render pipeline on a 2,000-page generated PDF.
///
/// Spawns a worker, opens a 2,000-page PDF, and renders tiles at different
/// scroll positions. Measures end-to-end: viewport → scheduling → dispatch → worker → TILE_READY.
fn bench_render_scroll_2000_pages(c: &mut Criterion) {
    let pdf_path = generate_temp_pdf(2000, "scroll");
    let worker = worker_path();

    let mut group = c.benchmark_group("render_scroll_2000p");
    group.sample_size(20);
    group.measurement_time(Duration::from_secs(30));

    group.bench_function("render_tiles_at_5_positions", |b| {
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                let doc_file = std::fs::File::open(&pdf_path).unwrap();
                let region = SharedRegion::create(TILE_RGBA8_BYTES * 4).unwrap();
                let mut child = spawn_worker_with_attachments(
                    &worker,
                    &SpawnAttachments { doc: Some(&doc_file), shmem: Some(region.file()), password: None },
                    &[],
                )
                .unwrap();

                // Inspect first to get page count.
                let cmd = Command::Inspect { correlation_id: 1 };
                child.transport.send(&encode_command(&cmd)).unwrap();
                let _ = child.transport.recv_timeout(Duration::from_secs(10)).unwrap();

                let start = Instant::now();

                // Render tiles at 5 different scroll positions (pages 0, 500, 1000, 1500, 1999).
                for (page, cid) in [(0u32, 2), (500, 3), (1000, 4), (1500, 5), (1999, 6)] {
                    let cmd = Command::RenderTile {
                        correlation_id: cid,
                        page,
                        x: 0, y: 0, w: 256, h: 256,
                        scale: 1.0, generation: 1, slot_offset: 0, col: 0, row: 0,
                    };
                    child.transport.send(&encode_command(&cmd)).unwrap();
                    let reply = child.transport.recv_timeout(Duration::from_secs(10)).unwrap();
                    match decode_worker_event(&reply).unwrap() {
                        protocol::events::WorkerEvent::TileReady { .. } => {}
                        other => panic!("expected TileReady, got {other:?}"),
                    }
                }

                let elapsed = start.elapsed();

                let _ = child.transport.send(b"CMD:QUIT\n");
                let _ = child.child.wait();
                total += elapsed;
            }
            total
        });
    });

    group.finish();

    // Clean up temp file.
    let _ = std::fs::remove_file(&pdf_path);
}

/// Benchmark: page positioning for large documents.
///
/// Measures how fast PagePositioner can compute visible regions for
/// various scroll positions in a 2,000-page document.
fn bench_page_positioning(c: &mut Criterion) {
    let mut group = c.benchmark_group("page_positioning_2000p");
    group.sample_size(100);

    let geos: Vec<PageGeometry> = (0..2000)
        .map(|_| PageGeometry { width: 612.0, height: 792.0, rotation: 0 })
        .collect();
    let pos = PagePositioner::new(geos);

    let mut state = ViewportState::new(800.0, 600.0);
    state.layout = render_pipeline::layout::PageLayout::Continuous;

    for (label, scroll_y) in &[
        ("top", 0.0),
        ("10pct", 150_000.0),
        ("50pct", 750_000.0),
        ("90pct", 1_350_000.0),
        ("bottom", 1_500_000.0),
    ] {
        state.scroll_y = *scroll_y;
        group.bench_function(format!("visible_regions_{label}"), |b| {
            b.iter(|| pos.compute_visible_regions(&state));
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_viewport_scroll_2000_pages,
    bench_render_scroll_2000_pages,
    bench_page_positioning,
);
criterion_main!(benches);
