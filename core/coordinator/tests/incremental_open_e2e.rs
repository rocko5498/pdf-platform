//! Editing a document another tool updated incrementally. [FR-VIEW-2, FR-STAMP]
//!
//! An incremental update writes a cross-reference section listing only the
//! objects it changed and points at the previous one with `/Prev`. Everything
//! signed, commented or filled by another application reaches us in that shape.
//! The worker read only the newest section, so fetching an object the update
//! left alone — every page of a document whose only change was a signature —
//! failed with "object N not found", and any edit that needs the page object
//! (stamp, OCR, forms, organize) failed with it.
//!
//! This product's own writer emits a complete table each time, so no test built
//! from our own output could see it.

mod common;

use std::io::Write as _;

use common::{scratch, worker_path};
use coordinator::document::DocumentCoordinator;

/// A two-object-deep document, then a compact update that touches one object.
fn incrementally_updated_pdf() -> Vec<u8> {
    let content = b"BT /F1 24 Tf 72 700 Td (PAGEMARK1) Tj ET";
    let objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
          /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>"
            .to_vec(),
        [
            format!("<< /Length {} >>\nstream\n", content.len()).as_bytes(),
            content.as_slice(),
            b"\nendstream",
        ]
        .concat(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
    ];

    let mut bytes: Vec<u8> = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        write!(bytes, "{} 0 obj\n", index + 1).unwrap();
        bytes.extend_from_slice(body);
        bytes.extend_from_slice(b"\nendobj\n");
    }
    let first_xref = bytes.len();
    write!(bytes, "xref\n0 {}\n", objects.len() + 1).unwrap();
    bytes.extend_from_slice(b"0000000000 65535 f \n");
    for offset in &offsets {
        writeln!(bytes, "{offset:010} 00000 n ").unwrap();
    }
    write!(
        bytes,
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{first_xref}\n%%EOF\n",
        objects.len() + 1
    )
    .unwrap();

    // The update: a new document-information object, and nothing else — the
    // page, its content and its font all stay where they were.
    let info_at = bytes.len();
    bytes.extend_from_slice(b"6 0 obj\n<< /Producer (Another Tool) >>\nendobj\n");
    let second_xref = bytes.len();
    bytes.extend_from_slice(b"xref\n6 1\n");
    writeln!(bytes, "{info_at:010} 00000 n ").unwrap();
    write!(
        bytes,
        "trailer\n<< /Size 7 /Root 1 0 R /Info 6 0 R /Prev {first_xref} >>\n\
         startxref\n{second_xref}\n%%EOF\n"
    )
    .unwrap();
    bytes
}

#[test]
fn a_page_of_an_incrementally_updated_document_can_be_read_and_edited() {
    let dir = scratch("incremental-open");
    let source = dir.join("updated.pdf");
    std::fs::write(&source, incrementally_updated_pdf()).expect("write source");

    let mut coord = DocumentCoordinator::open(&worker_path(), &source).expect("open");

    // The page object lives in the *first* section; only /Info was rewritten.
    let (page_obj_num, page_bytes) = coord
        .page_object(0)
        .expect("a page an incremental update did not touch must still be readable");
    assert_eq!(page_obj_num, 3);
    assert!(
        String::from_utf8_lossy(&page_bytes).contains("/Type /Page"),
        "resolved the wrong object: {:?}",
        String::from_utf8_lossy(&page_bytes)
    );

    // And an edit that depends on it goes through end to end.
    coord
        .apply_stamp(&pdf_model::stamp::Stamp {
            text: "REVIEWED".into(),
            ..pdf_model::stamp::Stamp::default()
        })
        .expect("stamp a page from an earlier section");
    let stamped = dir.join("stamped.pdf");
    coord.save_incremental(&stamped).expect("save");
    let _ = coord.close();

    let saved = std::fs::read(&stamped).expect("read stamped");
    assert!(
        saved.windows(b"REVIEWED".len()).any(|w| w == b"REVIEWED"),
        "the stamp never reached the file"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
