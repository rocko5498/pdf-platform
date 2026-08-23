//! Assembly moves the *right* pages, not just the right number of them.
//! [FR-MERGE-1, FR-SPLIT-1, FR-EXTRACT-1]
//!
//! `assembly_ops`' own tests assert page counts: merging pages 2–4 of a 5-page
//! document twice must yield 6 pages. It does — and it would also pass if the
//! operation silently took pages 1–3, or reversed them, or duplicated page 1
//! six times. Nothing checked which pages came out.
//!
//! Each page here carries its own number as drawn text, so the output can be
//! read back through the engine and compared to what was asked for.

use std::path::{Path, PathBuf};

use coordinator::document::DocumentCoordinator;

fn worker_path() -> PathBuf {
    let exe = std::env::current_exe().expect("current exe");
    let mut dir = exe.parent().expect("exe dir").to_path_buf();
    if dir.ends_with("deps") {
        dir.pop();
    }
    dir.join(if cfg!(windows) { "worker.exe" } else { "worker" })
}

/// Whether this run must prove assembly works rather than tolerate a missing
/// qpdf. CI installs it and sets this; a contributor without it still gets the
/// rest of the suite. [ADR-022, GR-8]
fn qpdf_required() -> bool {
    std::env::var_os("PDF_PLATFORM_REQUIRE_QPDF").is_some()
}

/// A document whose every page says which page it is.
fn numbered_pdf(pages: u32) -> Vec<u8> {
    let mut objects: Vec<Vec<u8>> = Vec::new();
    objects.push(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());

    // Object numbering: 1 catalog, 2 pages tree, 3 font, then per page a page
    // object and its content stream.
    let kids: Vec<String> = (0..pages).map(|i| format!("{} 0 R", 4 + i * 2)).collect();
    objects.push(
        format!(
            "<< /Type /Pages /Kids [{}] /Count {pages} >>",
            kids.join(" ")
        )
        .into_bytes(),
    );
    objects.push(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec());

    for index in 0..pages {
        let content_obj = 5 + index * 2;
        objects.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
                 /Resources << /Font << /F1 3 0 R >> >> /Contents {content_obj} 0 R >>"
            )
            .into_bytes(),
        );
        let content = format!(
            "BT\n/F1 24 Tf\n72 700 Td\n(PAGEMARK{}) Tj\nET\n",
            index + 1
        );
        objects.push(
            [
                format!("<< /Length {} >>\nstream\n", content.len()).as_bytes(),
                content.as_bytes(),
                b"endstream",
            ]
            .concat(),
        );
    }

    let mut bytes: Vec<u8> = b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        bytes.extend_from_slice(body);
        bytes.extend_from_slice(b"\nendobj\n");
    }
    let xref_at = bytes.len();
    bytes.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
    bytes.extend_from_slice(b"0000000000 65535 f \n");
    for offset in &offsets {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    bytes
}

/// The page markers a document's pages carry, in order.
fn page_markers(path: &Path) -> Vec<String> {
    let mut coord = DocumentCoordinator::open(&worker_path(), path)
        .unwrap_or_else(|error| panic!("open {}: {error}", path.display()));
    let count = coord.page_count();
    let mut markers = Vec::new();
    for index in 0..count {
        let model = coord
            .get_page_text(index)
            .unwrap_or_else(|error| panic!("extract page {index}: {error}"));
        let text: String = model.lines.iter().map(|line| line.text.as_str()).collect();
        let marker = text
            .split_whitespace()
            .find(|token| token.starts_with("PAGEMARK"))
            .unwrap_or("<none>")
            .to_owned();
        markers.push(marker);
    }
    let _ = coord.close();
    markers
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pdf-platform-assembly-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

#[test]
fn merging_a_page_range_takes_the_pages_that_were_asked_for() {
    use pdf_model::assembly_ops::{merge_pdfs_with_ranges, qpdf_available, PageRange};

    if !qpdf_available() {
        assert!(!qpdf_required(), "PDF_PLATFORM_REQUIRE_QPDF is set but qpdf is missing");
        eprintln!("skip: qpdf not on PATH");
        return;
    }

    let dir = temp_dir("range");
    let source = dir.join("source.pdf");
    std::fs::write(&source, numbered_pdf(5)).expect("write source");
    let output = dir.join("ranged.pdf");

    // Pages 2..=4, twice.
    let ranges = vec![PageRange::range(&source, 2, 4), PageRange::range(&source, 2, 4)];
    merge_pdfs_with_ranges(&ranges, &output).expect("merge ranges");

    let markers = page_markers(&output);
    assert_eq!(
        markers,
        vec![
            "PAGEMARK2", "PAGEMARK3", "PAGEMARK4", "PAGEMARK2", "PAGEMARK3", "PAGEMARK4"
        ],
        "the merge produced the right page count but the wrong pages"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn splitting_into_chunks_keeps_pages_in_order() {
    use pdf_model::assembly_ops::{qpdf_available, split_pdf_chunked};

    if !qpdf_available() {
        assert!(!qpdf_required(), "PDF_PLATFORM_REQUIRE_QPDF is set but qpdf is missing");
        eprintln!("skip: qpdf not on PATH");
        return;
    }

    let dir = temp_dir("chunk");
    let source = dir.join("source.pdf");
    std::fs::write(&source, numbered_pdf(5)).expect("write source");
    let out_dir = dir.join("parts");
    std::fs::create_dir_all(&out_dir).expect("create out dir");

    let parts = split_pdf_chunked(&source, 2, &out_dir).expect("split");
    assert_eq!(parts.len(), 3, "5 pages in chunks of 2 is 3 parts: {parts:?}");

    // Chunk boundaries are where an off-by-one hides: 1-2, 3-4, 5.
    assert_eq!(page_markers(&parts[0]), vec!["PAGEMARK1", "PAGEMARK2"]);
    assert_eq!(page_markers(&parts[1]), vec!["PAGEMARK3", "PAGEMARK4"]);
    assert_eq!(page_markers(&parts[2]), vec!["PAGEMARK5"]);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn extracting_a_page_range_takes_that_range() {
    use pdf_model::assembly_ops::{extract_pages, qpdf_available};

    if !qpdf_available() {
        assert!(!qpdf_required(), "PDF_PLATFORM_REQUIRE_QPDF is set but qpdf is missing");
        eprintln!("skip: qpdf not on PATH");
        return;
    }

    let dir = temp_dir("extract");
    let source = dir.join("source.pdf");
    std::fs::write(&source, numbered_pdf(5)).expect("write source");
    let output = dir.join("extracted.pdf");

    extract_pages(&source, 3, 4, &output).expect("extract");
    assert_eq!(
        page_markers(&output),
        vec!["PAGEMARK3", "PAGEMARK4"],
        "extraction took the wrong pages"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
