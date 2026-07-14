//! Micro-benchmarks for viewport layout and scheduling. [ADR-023, SDS §14]
//!
//! Measures:
//! - Viewport decomposition into tile requests
//! - PagePositioner compute_visible_regions
//! - Scale bucketing
//! - Velocity-aware prefetch margin computation
//!
//! Run: `cargo bench -p benchmarks --bench viewport`

use criterion::{criterion_group, criterion_main, Criterion};
use render_pipeline::layout::{PageGeometry, PageLayout, PagePositioner, ViewportState};
use render_pipeline::scheduler::{Viewport, ViewportRegion};

fn bench_viewport_decompose(c: &mut Criterion) {
    let mut group = c.benchmark_group("viewport_decompose");

    // Single page, 256x256 viewport
    let vp_single = Viewport {
        regions: vec![ViewportRegion { page: 0, x: 0, y: 0, w: 256, h: 256 }],
        scale: 1.0,
        rotation: 0,
        page_width: 612,
        page_height: 792,
    };
    group.bench_function("single_page_256x256", |b| {
        b.iter(|| vp_single.decompose(1));
    });

    // Large viewport covering multiple tiles
    let vp_large = Viewport {
        regions: vec![ViewportRegion { page: 0, x: 0, y: 0, w: 1024, h: 1024 }],
        scale: 1.0,
        rotation: 0,
        page_width: 612,
        page_height: 792,
    };
    group.bench_function("large_viewport_1024x1024", |b| {
        b.iter(|| vp_large.decompose(1));
    });

    // With prefetch
    group.bench_function("with_prefetch_margin_2", |b| {
        b.iter(|| vp_single.decompose_with_prefetch(1, 2));
    });

    group.finish();
}

fn bench_visible_regions(c: &mut Criterion) {
    let mut group = c.benchmark_group("visible_regions");

    // 100 pages, continuous layout
    let geos: Vec<PageGeometry> = (0..100)
        .map(|_| PageGeometry { width: 612.0, height: 792.0, rotation: 0 })
        .collect();
    let pos = PagePositioner::new(geos);

    let mut state = ViewportState::new(800.0, 600.0);
    state.layout = PageLayout::Continuous;

    group.bench_function("100_pages_continuous", |b| {
        b.iter(|| pos.compute_visible_regions(&state));
    });

    // Scroll to middle
    state.scroll_y = 40000.0;
    group.bench_function("100_pages_scrolled", |b| {
        b.iter(|| pos.compute_visible_regions(&state));
    });

    group.finish();
}

fn bench_scale_bucketing(c: &mut Criterion) {
    let mut group = c.benchmark_group("scale_bucketing");

    group.bench_function("bucket_scale_1_0", |b| {
        b.iter(|| render_pipeline::layout::bucket_scale(1.0));
    });

    group.bench_function("bucket_scale_1_37", |b| {
        b.iter(|| render_pipeline::layout::bucket_scale(1.37));
    });

    group.finish();
}

fn bench_prefetch_margin(c: &mut Criterion) {
    let mut group = c.benchmark_group("prefetch_margin");

    group.bench_function("at_rest", |b| {
        b.iter(|| render_pipeline::layout::compute_prefetch_margin(0.0, 3.0, 2, 8));
    });

    group.bench_function("fast_scroll", |b| {
        b.iter(|| render_pipeline::layout::compute_prefetch_margin(2000.0, 3.0, 2, 8));
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_viewport_decompose,
    bench_visible_regions,
    bench_scale_bucketing,
    bench_prefetch_margin,
);
criterion_main!(benches);
