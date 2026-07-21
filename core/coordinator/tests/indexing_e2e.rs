//! End-to-end cross-document indexing test: real enrollment bounds, real
//! worker-based text extraction, real Tantivy backend. [ADR-019 §3, ADR-034]

use std::io::Write as _;
use std::path::PathBuf;

use coordinator::broker::IndexEnrollmentRegistry;
use coordinator::indexing::{
    indexing_summary, reindex_enrollment, remove_enrollment_files, FileIndexRegistry,
};
use search::tantivy_backend::CrossDocumentIndex;

fn worker_path() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let target_dir = manifest_dir.parent().unwrap().join("target");
    let name = if cfg!(windows) { "worker.exe" } else { "worker" };
    let debug = target_dir.join("debug").join(name);
    let release = target_dir.join("release").join(name);
    match (debug.exists(), release.exists()) {
        (true, true) => {
            let d = std::fs::metadata(&debug).and_then(|m| m.modified()).ok();
            let r = std::fs::metadata(&release).and_then(|m| m.modified()).ok();
            if r >= d {
                release
            } else {
                debug
            }
        }
        (true, false) => debug,
        (false, true) => release,
        (false, false) => panic!(
            "worker binary not found at {} or {}",
            debug.display(),
            release.display()
        ),
    }
}

/// A real one-page PDF with an actual extractable content stream (unlike
/// the blank fixtures used elsewhere in this workspace) — text indexing has
/// nothing to find on a contentless page.
fn one_page_pdf_with_text(text: &str) -> Vec<u8> {
    let content = format!("BT /F1 24 Tf 72 700 Td ({text}) Tj ET");
    let mut bytes = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();

    offsets.push(bytes.len());
    bytes.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
    offsets.push(bytes.len());
    bytes.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n");
    offsets.push(bytes.len());
    bytes.extend_from_slice(b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>\nendobj\n");
    offsets.push(bytes.len());
    write!(
        bytes,
        "4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n",
        content.len()
    )
    .unwrap();
    offsets.push(bytes.len());
    bytes.extend_from_slice(
        b"5 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>\nendobj\n",
    );

    let xref = bytes.len();
    write!(bytes, "xref\n0 {}\n", offsets.len() + 1).unwrap();
    bytes.extend_from_slice(b"0000000000 65535 f \n");
    for offset in &offsets {
        writeln!(bytes, "{offset:010} 00000 n ").unwrap();
    }
    write!(
        bytes,
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        offsets.len() + 1
    )
    .unwrap();
    bytes
}

#[test]
fn reindex_enrollment_scans_extracts_and_indexes_real_text() {
    let worker = worker_path();
    let root = std::env::temp_dir().join(format!(
        "pdf-platform-indexing-e2e-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let doc_path = root.join("report.pdf");
    std::fs::write(&doc_path, one_page_pdf_with_text("Hello World")).unwrap();

    let index_dir = root.join(".index");
    let mut index = CrossDocumentIndex::open_or_create(&index_dir).unwrap();
    let mut file_registry = FileIndexRegistry::new();
    let mut enrollment_registry = IndexEnrollmentRegistry::new();
    let enrollment = enrollment_registry.enroll(&root).unwrap();

    // First pass: should find and index the one file.
    let report = reindex_enrollment(
        &worker,
        &enrollment_registry,
        enrollment,
        &root,
        &mut file_registry,
        &mut index,
    );
    assert_eq!(report.files_scanned, 1);
    assert_eq!(report.files_reindexed, 1);
    assert_eq!(report.files_skipped_unchanged, 0);
    assert!(report.errors.is_empty(), "unexpected errors: {:?}", report.errors);

    let hits = index.search("Hello", 10).unwrap();
    assert_eq!(hits.len(), 1, "indexed text should be findable");

    // Second pass, nothing changed: must skip, not re-extract.
    let report2 = reindex_enrollment(
        &worker,
        &enrollment_registry,
        enrollment,
        &root,
        &mut file_registry,
        &mut index,
    );
    assert_eq!(report2.files_reindexed, 0);
    assert_eq!(report2.files_skipped_unchanged, 1);

    // Modify the file: must be picked up again (file-change invalidation).
    std::fs::write(&doc_path, one_page_pdf_with_text("Goodbye World")).unwrap();
    let report3 = reindex_enrollment(
        &worker,
        &enrollment_registry,
        enrollment,
        &root,
        &mut file_registry,
        &mut index,
    );
    assert_eq!(report3.files_reindexed, 1, "changed file must be reindexed");
    assert!(
        index.search("Hello", 10).unwrap().is_empty(),
        "stale text must be replaced, not merely appended"
    );
    assert_eq!(index.search("Goodbye", 10).unwrap().len(), 1);

    let summary = indexing_summary(&file_registry, &index, &index_dir);
    assert_eq!(summary.tracked_file_count, 1);
    assert!(summary.disk_size_bytes > 0);

    // Enrollment removal must remove the file's documents from the index.
    let removed = remove_enrollment_files(&mut file_registry, &mut index, &root).unwrap();
    assert_eq!(removed, 1);
    assert!(index.search("Goodbye", 10).unwrap().is_empty());
    assert_eq!(file_registry.tracked_file_count(), 0);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn reindex_enrollment_rejects_files_outside_the_enrolled_root() {
    let worker = worker_path();
    let base = std::env::temp_dir().join(format!(
        "pdf-platform-indexing-e2e-outside-{}",
        std::process::id()
    ));
    let enrolled = base.join("enrolled");
    let outside = base.join("outside");
    std::fs::create_dir_all(&enrolled).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(&outside.join("sneaky.pdf"), one_page_pdf_with_text("Sneaky")).unwrap();

    let index_dir = base.join(".index");
    let mut index = CrossDocumentIndex::open_or_create(&index_dir).unwrap();
    let mut file_registry = FileIndexRegistry::new();
    let mut enrollment_registry = IndexEnrollmentRegistry::new();
    let enrollment = enrollment_registry.enroll(&enrolled).unwrap();

    // Nothing under `enrolled/` — the file outside it must never be reached
    // even though it lives in a sibling directory under the same base.
    let report = reindex_enrollment(
        &worker,
        &enrollment_registry,
        enrollment,
        &enrolled,
        &mut file_registry,
        &mut index,
    );
    assert_eq!(report.files_scanned, 0);
    assert!(index.search("Sneaky", 10).unwrap().is_empty());

    let _ = std::fs::remove_dir_all(base);
}
