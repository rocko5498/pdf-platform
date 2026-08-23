//! Editing a document written the way current producers write them.
//! [FR-VIEW-2, FR-STAMP, SDS §3.1]
//!
//! A PDF 1.5+ file keeps its cross-reference in a compressed stream and most of
//! its objects inside object streams. Until this week our COS layer could read
//! neither, so opening such a file reported no pages and every edit failed —
//! while PDFium rendered it perfectly, which is why it looked fine.
//!
//! `pdf-cos` now has unit coverage for both. This drives the whole product
//! path instead: real worker, real IPC, open → stamp → save → reopen.

mod common;

use std::io::Write as _;

use common::{scratch, worker_path};
use coordinator::document::DocumentCoordinator;
use flate2::write::ZlibEncoder;
use flate2::Compression;

fn deflate(data: &[u8]) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data).expect("compress");
    encoder.finish().expect("finish")
}

/// One page with visible text, its objects inside an object stream, indexed by
/// a cross-reference stream.
fn modern_pdf() -> Vec<u8> {
    let content = b"BT /F1 24 Tf 72 700 Td (PAGEMARK1) Tj ET";

    let mut bytes: Vec<u8> = b"%PDF-1.6\n".to_vec();

    // The content stream and the font stay ordinary objects: a content stream
    // cannot live in an object stream (PDF 32000-1 7.5.7).
    let content_at = bytes.len();
    write!(bytes, "4 0 obj\n<< /Length {} >>\nstream\n", content.len()).unwrap();
    bytes.extend_from_slice(content);
    bytes.extend_from_slice(b"\nendstream\nendobj\n");

    let font_at = bytes.len();
    bytes.extend_from_slice(
        b"5 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n",
    );

    // Catalog, page tree and page live in object stream 6.
    let payload: Vec<(u32, String)> = vec![
        (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into()),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
             /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>"
                .into(),
        ),
    ];
    let mut header = String::new();
    let mut body = String::new();
    for (num, text) in &payload {
        header.push_str(&format!("{num} {} ", body.len()));
        body.push_str(text);
        body.push(' ');
    }
    let first = header.len();
    let compressed = deflate(format!("{header}{body}").as_bytes());

    let objstm_at = bytes.len();
    write!(
        bytes,
        "6 0 obj\n<< /Type /ObjStm /N {} /First {first} /Filter /FlateDecode /Length {} >>\nstream\n",
        payload.len(),
        compressed.len()
    )
    .unwrap();
    bytes.extend_from_slice(&compressed);
    bytes.extend_from_slice(b"\nendstream\nendobj\n");

    // Object 7 is the cross-reference stream.
    let xref_at = bytes.len();
    let mut table = Vec::new();
    table.push(0u8);
    table.extend_from_slice(&0u32.to_be_bytes());
    table.extend_from_slice(&65535u16.to_be_bytes());
    for index in 0..3u16 {
        table.push(2u8);
        table.extend_from_slice(&6u32.to_be_bytes());
        table.extend_from_slice(&index.to_be_bytes());
    }
    for offset in [content_at, font_at, objstm_at, xref_at] {
        table.push(1u8);
        table.extend_from_slice(&(offset as u32).to_be_bytes());
        table.extend_from_slice(&0u16.to_be_bytes());
    }
    let compressed_table = deflate(&table);
    write!(
        bytes,
        "7 0 obj\n<< /Type /XRef /Size 8 /W [1 4 2] /Root 1 0 R /Filter /FlateDecode /Length {} >>\nstream\n",
        compressed_table.len()
    )
    .unwrap();
    bytes.extend_from_slice(&compressed_table);
    bytes.extend_from_slice(b"\nendstream\nendobj\n");
    write!(bytes, "startxref\n{xref_at}\n%%EOF\n").unwrap();
    bytes
}

#[test]
fn a_modern_document_opens_with_its_pages() {
    let dir = scratch("modern-open");
    let path = dir.join("modern.pdf");
    std::fs::write(&path, modern_pdf()).expect("write");

    let mut coord = DocumentCoordinator::open(&worker_path(), &path).expect("open modern document");
    let pages = coord.page_count();
    let joined: String = coord
        .get_page_text(0)
        .expect("extract")
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect();
    let _ = coord.close();

    assert_eq!(pages, 1, "a document whose page tree is in an object stream");
    assert!(
        joined.contains("PAGEMARK1"),
        "the page's own text must come back: {joined:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_modern_document_can_be_stamped_and_saved() {
    let dir = scratch("modern-stamp");
    let path = dir.join("modern.pdf");
    std::fs::write(&path, modern_pdf()).expect("write");

    let mut coord = DocumentCoordinator::open(&worker_path(), &path).expect("open");
    let before = coord.render_page(0, 0.5).expect("render before");

    // The page object this stamps lives inside the object stream: fetching it
    // is what used to fail with "object 3 not found".
    coord
        .apply_stamp(&pdf_model::stamp::Stamp {
            text: "REVIEWED".into(),
            ..pdf_model::stamp::Stamp::default()
        })
        .expect("stamp a page that lives in an object stream");
    let stamped = dir.join("stamped.pdf");
    coord.save_incremental(&stamped).expect("save");
    let _ = coord.close();

    let mut reopened = DocumentCoordinator::open(&worker_path(), &stamped).expect("reopen");
    let after = reopened.render_page(0, 0.5).expect("render after");
    let joined: String = reopened
        .get_page_text(0)
        .expect("extract")
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect();
    let _ = reopened.close();
    assert!(
        joined.contains("REVIEWED"),
        "the stamp must be on the reopened page: {joined:?}"
    );
    assert!(
        joined.contains("PAGEMARK1"),
        "and the page's original text must survive: {joined:?}"
    );
    assert_ne!(
        before.pixels, after.pixels,
        "the stamped page must render differently"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
