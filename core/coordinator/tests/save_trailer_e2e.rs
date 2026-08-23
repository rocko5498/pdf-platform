//! Saving must keep pointing at the document's own catalog. [FR-SAVE, SDS §3.3]
//!
//! The incremental writer wrote `/Root 1 0 R` into every trailer, and the
//! coordinator read object 1 to find the page tree, falling back to object 2.
//! Both assumed the catalog is object 1. The format does not say that, and
//! producers that number the catalog last are ordinary.
//!
//! On such a document the effects were: stamping failed with "stamping nested
//! page trees is not yet supported" — a complaint about a shape the document
//! did not have — and saving produced a file whose newest trailer pointed at a
//! content stream, so reopening it found no pages. Every fixture in this
//! repository numbered the catalog 1, so nothing saw it.

mod common;

use std::io::Write as _;

use common::{scratch, worker_path};
use coordinator::document::DocumentCoordinator;

/// A one-page document whose catalog is object 5 and whose object 1 is a
/// content stream, with `/Info` and `/ID` in the trailer.
fn catalog_numbered_last() -> Vec<u8> {
    let content = b"BT /F1 24 Tf 72 700 Td (PAGEMARK1) Tj ET";
    let objects: Vec<Vec<u8>> = vec![
        [
            format!("<< /Length {} >>\nstream\n", content.len()).as_bytes(),
            content.as_slice(),
            b"\nendstream",
        ]
        .concat(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        b"<< /Type /Page /Parent 4 0 R /MediaBox [0 0 612 792] \
          /Resources << /Font << /F1 2 0 R >> >> /Contents 1 0 R >>"
            .to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Catalog /Pages 4 0 R >>".to_vec(),
        b"<< /Producer (Another Tool) >>".to_vec(),
    ];

    let mut bytes: Vec<u8> = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        write!(bytes, "{} 0 obj\n", index + 1).unwrap();
        bytes.extend_from_slice(body);
        bytes.extend_from_slice(b"\nendobj\n");
    }
    let xref = bytes.len();
    write!(bytes, "xref\n0 {}\n", objects.len() + 1).unwrap();
    bytes.extend_from_slice(b"0000000000 65535 f \n");
    for offset in &offsets {
        writeln!(bytes, "{offset:010} 00000 n ").unwrap();
    }
    write!(
        bytes,
        "trailer\n<< /Size {} /Root 5 0 R /Info 6 0 R \
         /ID [<0123456789ABCDEF0123456789ABCDEF> <0123456789ABCDEF0123456789ABCDEF>] >>\n\
         startxref\n{xref}\n%%EOF\n",
        objects.len() + 1
    )
    .unwrap();
    bytes
}

/// The last trailer dictionary in a file.
fn last_trailer(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let at = text.rfind("trailer").expect("a trailer");
    let end = text[at..].find(">>").expect("a trailer dictionary end") + at + 2;
    text[at..end].to_string()
}

#[test]
fn a_saved_document_still_names_its_own_catalog() {
    let dir = scratch("save-trailer-root");
    let source = dir.join("source.pdf");
    std::fs::write(&source, catalog_numbered_last()).expect("write source");

    let mut coord = DocumentCoordinator::open(&worker_path(), &source).expect("open");
    coord
        .apply_stamp(&pdf_model::stamp::Stamp {
            text: "REVIEWED".into(),
            ..pdf_model::stamp::Stamp::default()
        })
        .expect("a flat page tree whose catalog is object 5 is not 'nested'");
    let saved = dir.join("saved.pdf");
    coord.save_incremental(&saved).expect("save");
    let _ = coord.close();

    let bytes = std::fs::read(&saved).expect("read saved");
    let trailer = last_trailer(&bytes);
    assert!(
        trailer.contains("/Root 5 0 R"),
        "the trailer must name the document's own catalog, got {trailer}"
    );

    let mut reopened = DocumentCoordinator::open(&worker_path(), &saved).expect("reopen");
    let pages = reopened.page_count();
    let _ = reopened.close();
    assert_eq!(pages, 1, "the saved document lost its pages");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn saving_carries_the_trailer_forward() {
    // `/Info` and `/ID` were dropped by every save. `/ID` identifies the file
    // for signatures and update chains; re-deriving one makes it a different
    // document.
    let dir = scratch("save-trailer-carry");
    let source = dir.join("source.pdf");
    std::fs::write(&source, catalog_numbered_last()).expect("write source");

    let mut coord = DocumentCoordinator::open(&worker_path(), &source).expect("open");
    coord.rotate_pages(&[0], 90).expect("rotate");
    let saved = dir.join("saved.pdf");
    coord.save_incremental(&saved).expect("save");
    let _ = coord.close();

    let trailer = last_trailer(&std::fs::read(&saved).expect("read saved"));
    assert!(
        trailer.contains("/Info 6 0 R"),
        "the document information dictionary was dropped: {trailer}"
    );
    assert!(
        trailer.contains("/ID [<0123456789ABCDEF0123456789ABCDEF>"),
        "the file identifier was dropped or re-derived: {trailer}"
    );
    assert!(
        trailer.contains("/Prev"),
        "the update must chain to the section it updates: {trailer}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
