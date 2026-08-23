//! `page_meta` must report the page's real rotation. [FR-ORG-2, FR-VIEW-1]
//!
//! It used to return a hard-coded `0` for every page of every document, with a
//! comment claiming pdfium-render did not expose rotation — it does, via
//! `PdfPage::rotation()`. Nothing noticed, because nothing had ever asked the
//! engine about a rotated page: the product's own rotate command patches
//! `/Rotate` in the page dictionary and never reads it back through here.

use std::io::Write as _;

use engine_api::structure::Structure;
use engine_pdfium::PdfiumEngine;

/// A one-page document whose page carries `/Rotate {degrees}`.
fn rotated_pdf(degrees: u32) -> Vec<u8> {
    let content = b"BT /F1 24 Tf 72 700 Td (rotated) Tj ET";
    let rotate = if degrees == 0 {
        String::new()
    } else {
        format!("/Rotate {degrees} ")
    };

    let objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] {rotate}\
             /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>"
        )
        .into_bytes(),
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
    let xref = bytes.len();
    write!(bytes, "xref\n0 {}\n", objects.len() + 1).unwrap();
    bytes.extend_from_slice(b"0000000000 65535 f \n");
    for offset in &offsets {
        writeln!(bytes, "{offset:010} 00000 n ").unwrap();
    }
    write!(
        bytes,
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        objects.len() + 1
    )
    .unwrap();
    bytes
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("pdf-platform-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

#[test]
fn page_meta_reports_the_rotation_the_page_declares() {
    let dir = scratch("engine-rotation");
    for degrees in [0u32, 90, 180, 270] {
        let path = dir.join(format!("rotate-{degrees}.pdf"));
        std::fs::write(&path, rotated_pdf(degrees)).expect("write fixture");

        let engine = PdfiumEngine::from_file(&path, None).expect("open fixture");
        let meta = engine.page_meta().expect("page metadata");

        assert_eq!(meta.len(), 1);
        assert_eq!(
            meta[0].rotation, degrees,
            "a page declaring /Rotate {degrees} was reported as {} degrees",
            meta[0].rotation
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_quarter_turn_swaps_the_reported_page_size() {
    // Rotation and dimensions must agree: a viewer that lays a page out from
    // width/height and orients it from `rotation` needs both to describe the
    // same page.
    let dir = scratch("engine-rotation-size");
    let upright_path = dir.join("upright.pdf");
    let turned_path = dir.join("turned.pdf");
    std::fs::write(&upright_path, rotated_pdf(0)).expect("write upright");
    std::fs::write(&turned_path, rotated_pdf(90)).expect("write turned");

    let upright = PdfiumEngine::from_file(&upright_path, None).expect("open upright");
    let turned = PdfiumEngine::from_file(&turned_path, None).expect("open turned");
    let upright_meta = upright.page_meta().expect("upright metadata");
    let turned_meta = turned.page_meta().expect("turned metadata");

    assert_eq!(upright_meta[0].rotation, 0);
    assert_eq!(turned_meta[0].rotation, 90);
    assert!(
        (turned_meta[0].width - upright_meta[0].height).abs() < 0.5
            && (turned_meta[0].height - upright_meta[0].width).abs() < 0.5,
        "a quarter turn must swap width and height: upright {}x{}, turned {}x{}",
        upright_meta[0].width,
        upright_meta[0].height,
        turned_meta[0].width,
        turned_meta[0].height
    );

    let _ = std::fs::remove_dir_all(&dir);
}
