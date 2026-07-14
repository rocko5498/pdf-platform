//! Micro-benchmarks for the render pipeline. [ADR-023, SDS §14, MET-PERF]
//!
//! Measures in-process throughput:
//! - Stub engine rasterization throughput (tiles/sec, MB/sec)
//! - PDFium engine rasterization throughput
//! - TilePool allocation latency
//! - RenderTileRequest codec roundtrip
//! - TILE_READY codec roundtrip
//!
//! Cross-process IPC benchmark is in worker-main/tests/ipc_bench.rs.
//!
//! Run: `cargo bench -p benchmarks --bench render_pipeline`

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use engine_api::rasterize::{Rasterize, RasterizeRequest, TileRect};
use engine_stub::StubEngine;
use protocol::commands::{decode_render_tile, encode_render_tile, RenderTileRequest};
use protocol::handles::{
    decode_tile_ready, encode_tile_ready, PixelFormat, TileSlotDesc, TILE_RGBA8_BYTES,
};

fn bench_rasterize(c: &mut Criterion) {
    let mut group = c.benchmark_group("rasterize");
    let engine = StubEngine::new(10);

    for &(w, h) in &[(64, 64), (128, 128), (256, 256), (512, 512)] {
        let bytes = (w * h * 4) as u64;
        group.throughput(Throughput::Bytes(bytes));
        group.bench_with_input(
            BenchmarkId::new("stub", format!("{w}x{h}")),
            &(w, h),
            |b, &(w, h)| {
                b.iter(|| {
                    engine
                        .rasterize(&RasterizeRequest {
                            page_index: 0,
                            rect: TileRect { x: 0, y: 0, w, h },
                            scale: 1.0,
                        })
                        .unwrap()
                });
            },
        );
    }
    group.finish();
}

fn bench_rasterize_pdfium(c: &mut Criterion) {
    // Benchmark PDFium rendering with a minimal PDF.
    let pdf_bytes = b"%PDF-1.0\n\
          1 0 obj\n\
          <</Type /Catalog /Pages 2 0 R>>\n\
          endobj\n\
          2 0 obj\n\
          <</Type /Pages /Kids [3 0 R] /Count 1>>\n\
          endobj\n\
          3 0 obj\n\
          <</Type /Page /Parent 2 0 R /MediaBox [0 0 612 792]>>\n\
          endobj\n\
          xref\n\
          0 4\n\
          0000000000 65535 f \n\
          0000000009 00000 n \n\
          0000000056 00000 n \n\
          0000000111 00000 n \n\
          trailer\n\
          <</Size 4 /Root 1 0 R>>\n\
          startxref\n\
          180\n\
          %%EOF";

    let engine = match engine_pdfium::PdfiumEngine::from_bytes(pdf_bytes.to_vec()) {
        Ok(e) => e,
        Err(_) => {
            eprintln!("PDFium not available, skipping pdfium benchmarks");
            return;
        }
    };

    let mut group = c.benchmark_group("rasterize_pdfium");
    for &(w, h) in &[(64, 64), (128, 128), (256, 256), (512, 512)] {
        let bytes = (w * h * 4) as u64;
        group.throughput(Throughput::Bytes(bytes));
        group.bench_with_input(
            BenchmarkId::new("pdfium", format!("{w}x{h}")),
            &(w, h),
            |b, &(w, h)| {
                b.iter(|| {
                    engine
                        .rasterize(&RasterizeRequest {
                            page_index: 0,
                            rect: TileRect { x: 0, y: 0, w, h },
                            scale: 1.0,
                        })
                        .unwrap()
                });
            },
        );
    }
    group.finish();
}

fn bench_tile_pool_alloc(c: &mut Criterion) {
    let mut group = c.benchmark_group("tile_pool");
    group.bench_function("alloc_4slot", |b| {
        b.iter_batched(
            || render_pipeline::shmem::TilePool::create(4).unwrap(),
            |mut pool| {
                for gen in 0..4 {
                    pool.alloc_slot(gen);
                }
            },
            criterion::BatchSize::SmallInput,
        );
    });
    group.finish();
}

fn bench_render_tile_codec(c: &mut Criterion) {
    let mut group = c.benchmark_group("render_tile_codec");
    let req = RenderTileRequest {
        page: 0,
        x: 0,
        y: 0,
        w: 256,
        h: 256,
        scale: 1.0,
        generation: 1,
        slot_offset: 0,
    };

    group.bench_function("encode", |b| {
        b.iter(|| encode_render_tile(&req));
    });

    let encoded = encode_render_tile(&req);
    group.bench_function("decode", |b| {
        b.iter(|| decode_render_tile(&encoded).unwrap());
    });

    group.bench_function("roundtrip", |b| {
        b.iter(|| {
            let enc = encode_render_tile(&req);
            decode_render_tile(&enc).unwrap()
        });
    });

    group.finish();
}

fn bench_tile_ready_codec(c: &mut Criterion) {
    let mut group = c.benchmark_group("tile_ready_codec");
    let desc = TileSlotDesc {
        offset: 0,
        len: TILE_RGBA8_BYTES as u32,
        format: PixelFormat::Rgba8,
        generation: 42,
        page: 3,
        col: 1,
        row: 2,
    };

    group.bench_function("encode", |b| {
        b.iter(|| encode_tile_ready(&desc));
    });

    let encoded = encode_tile_ready(&desc);
    group.bench_function("decode", |b| {
        b.iter(|| decode_tile_ready(&encoded).unwrap());
    });

    group.bench_function("roundtrip", |b| {
        b.iter(|| {
            let enc = encode_tile_ready(&desc);
            decode_tile_ready(&enc).unwrap()
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_rasterize,
    bench_rasterize_pdfium,
    bench_tile_pool_alloc,
    bench_render_tile_codec,
    bench_tile_ready_codec,
);
criterion_main!(benches);
