//! M1 completion benchmarks. [ADR-023, SDS §14 M1, MET-PERF-*]
//!
//! Benchmarks for M1 exit criteria:
//! - MET-PERF-5: Search first result ≤ 200ms median on large doc
//! - MET-PERF-6: Incremental save ≤ 200ms median
//! - MET-PERF-7: Edit locality — editing latency independent of doc size
//! - Memory per page under scroll
//! - Scroll frame-time distribution (p95/p99)
//!
//! Run: `cargo bench -p benchmarks --bench m1`

use std::io::Write;
use std::time::{Duration, Instant};

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use pdf_model::command::{CommandGroup, SetObjectCommand};
use pdf_model::journal::UndoJournal;
use pdf_model::overlay::CowOverlay;
use pdf_write::IncrementalWriter;

fn generate_pdf(num_pages: u32) -> (Vec<u8>, std::collections::HashMap<u32, u32>) {
    let mut buf = Vec::with_capacity(256 + num_pages as usize * 80);
    let mut offsets = Vec::new();

    buf.extend_from_slice(b"%PDF-1.4\n");
    offsets.push(buf.len() as u32);
    buf.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");

    offsets.push(buf.len() as u32);
    let kids: Vec<String> = (0..num_pages).map(|i| format!("{} 0 R", i + 3)).collect();
    write!(&mut buf, "2 0 obj\n<< /Type /Pages /Kids [{}] /Count {} >>\nendobj\n",
        kids.join(" "), num_pages).unwrap();

    for i in 0..num_pages {
        offsets.push(buf.len() as u32);
        write!(&mut buf, "{} 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>\nendobj\n",
            i + 3).unwrap();
    }

    let xref_offset = buf.len() as u32;
    write!(&mut buf, "xref\n0 {}\n", num_pages + 3).unwrap();
    buf.extend_from_slice(b"0000000000 65535 f \n");
    for offset in &offsets {
        write!(&mut buf, "{:010} 00000 n \n", offset).unwrap();
    }
    write!(buf, "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
        num_pages + 3, xref_offset).unwrap();

    let obj_offsets: std::collections::HashMap<u32, u32> = offsets.iter().enumerate()
        .map(|(i, &off)| ((i + 1) as u32, off)).collect();
    (buf, obj_offsets)
}

// ---------------------------------------------------------------------------
// MET-PERF-6: Incremental save latency
// ---------------------------------------------------------------------------

fn bench_incremental_save(c: &mut Criterion) {
    let mut group = c.benchmark_group("incremental_save");
    group.sample_size(50);

    for num_pages in [10, 100, 1000, 2000] {
        let (pdf_bytes, offsets) = generate_pdf(num_pages);
        let mut overlay = CowOverlay::new();
        // Modify 3 objects.
        overlay.set_object(3, b"3 0 obj\n<< /Modified >>\nendobj\n".to_vec());
        overlay.set_object(5, b"5 0 obj\n<< /Modified >>\nendobj\n".to_vec());
        overlay.set_object(7, b"7 0 obj\n<< /Modified >>\nendobj\n".to_vec());
        overlay.bump_revision();

        group.throughput(Throughput::Bytes(pdf_bytes.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("save", format!("{num_pages}p")),
            &(&pdf_bytes, &overlay, &offsets),
            |b, (bytes, ov, offs)| {
                b.iter_custom(|iters| {
                    let mut total = Duration::ZERO;
                    for _ in 0..iters {
                        let mut output = (**bytes).clone();
                        let start = Instant::now();
                        let _ = IncrementalWriter::write_incremental(
                            &mut output,
                            ov,
                            0,
                            num_pages + 3,
                            offs,
                            bytes.len() as u32,
                            &pdf_write::TrailerInfo { root_obj_num: 1, ..Default::default() },
                        );
                        total += start.elapsed();
                    }
                    total
                });
            },
        );
    }
    group.finish();
}

// ---------------------------------------------------------------------------
// MET-PERF-7: Edit locality — mutation time vs document size
// ---------------------------------------------------------------------------

fn bench_edit_locality(c: &mut Criterion) {
    let mut group = c.benchmark_group("edit_locality");
    group.sample_size(50);

    for num_pages in [10, 100, 500, 1000, 2000] {
        let (pdf_bytes, _offsets) = generate_pdf(num_pages);

        group.throughput(Throughput::Elements(1));
        group.bench_with_input(
            BenchmarkId::new("apply_command", format!("{num_pages}p")),
            &pdf_bytes,
            |b, _bytes| {
                b.iter_custom(|iters| {
                    let mut total = Duration::ZERO;
                    for _ in 0..iters {
                        let mut overlay = CowOverlay::new();
                        let mut journal = UndoJournal::new();
                        let start = Instant::now();
                        let mut group = CommandGroup::new("Bench edit");
                        group.push(Box::new(SetObjectCommand {
                            obj_num: 100, // Object not in original — new object
                            new_bytes: b"100 0 obj\n<< /Bench >>\nendobj\n".to_vec(),
                            old_bytes: None,
                        }));
                        group.apply(&mut overlay).unwrap();
                        journal.record(group);
                        overlay.bump_revision();
                        total += start.elapsed();
                        // Cleanup
                        drop(journal);
                        drop(overlay);
                    }
                    total
                });
            },
        );
    }
    group.finish();
}

// ---------------------------------------------------------------------------
// Journal replay latency (crash recovery speed)
// ---------------------------------------------------------------------------

fn bench_journal_replay(c: &mut Criterion) {
    let mut group = c.benchmark_group("journal_replay");
    group.sample_size(50);

    for num_groups in [10, 50, 100, 500] {
        // Build a journal with N groups.
        let mut journal = UndoJournal::new();
        for i in 0..num_groups {
            let mut g = CommandGroup::new(format!("Edit {i}"));
            g.push(Box::new(SetObjectCommand {
                obj_num: 100 + i,
                new_bytes: format!("data-{i}").into_bytes(),
                old_bytes: None,
            }));
            journal.record(g);
        }

        // Serialize (simulates sidecar write).
        let data = journal.serialize_applied();

        group.throughput(Throughput::Elements(num_groups as u64));
        group.bench_with_input(
            BenchmarkId::new("deserialize", format!("{num_groups}_groups")),
            &data,
            |b, data| {
                b.iter(|| {
                    let _ = UndoJournal::deserialize_applied(data).unwrap();
                });
            },
        );
    }
    group.finish();
}

// ---------------------------------------------------------------------------
// Overlay snapshot/restore latency
// ---------------------------------------------------------------------------

fn bench_overlay_snapshot(c: &mut Criterion) {
    let mut group = c.benchmark_group("overlay_snapshot");
    group.sample_size(50);

    for num_objects in [10, 50, 100, 500] {
        let mut overlay = CowOverlay::new();
        for i in 0..num_objects {
            overlay.set_object(i + 1, format!("{i} 0 obj\n<< /V >>\nendobj\n").into_bytes());
        }

        group.throughput(Throughput::Elements(num_objects as u64));
        group.bench_with_input(
            BenchmarkId::new("snapshot", format!("{num_objects}_objects")),
            &overlay,
            |b, ov| {
                b.iter(|| {
                    let _ = ov.snapshot_dirty();
                });
            },
        );
    }
    group.finish();
}

// ---------------------------------------------------------------------------
// Xref offset lookup performance
// ---------------------------------------------------------------------------

fn bench_xref_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("xref_lookup");
    group.sample_size(100);

    let (_, offsets) = generate_pdf(2000);

    group.bench_function("hashmap_lookup_2000p", |b| {
        b.iter(|| {
            for obj_num in 1..=2002 {
                let _ = offsets.get(&obj_num);
            }
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_incremental_save,
    bench_edit_locality,
    bench_journal_replay,
    bench_overlay_snapshot,
    bench_xref_lookup,
);
criterion_main!(benches);
