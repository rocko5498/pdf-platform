//! A document's layers must reach the panel that lists them. [FR-VIEW-4, M1]
//!
//! `PdfiumEngine::layers` returns `Layers::default()` — an empty list — for
//! every document, because pdfium-render exposes no optional-content API. The
//! worker asked the engine, so a document with three layers and a document
//! with none answered identically: `has_layers=false`. The M1 tracker row read
//! "Outline / layers / attachments — Met".
//!
//! These drive the real worker over IPC with documents whose optional content
//! is known, because that is the only place the whole chain is exercised.

mod common;

use common::{scratch, worker_path};
use coordinator::document::DocumentCoordinator;

/// A one-page document declaring `groups`, each `(name, visible)`.
fn document_with_layers(groups: &[(&str, bool)]) -> Vec<u8> {
    use std::io::Write as _;

    let first_ocg = 4u32;
    let refs: Vec<String> = (0..groups.len())
        .map(|index| format!("{} 0 R", first_ocg + index as u32))
        .collect();
    let off: Vec<String> = groups
        .iter()
        .enumerate()
        .filter(|(_, (_, visible))| !visible)
        .map(|(index, _)| format!("{} 0 R", first_ocg + index as u32))
        .collect();

    let mut objects: Vec<Vec<u8>> = vec![
        format!(
            "<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [{}] \
             /D << /Order [{}] /OFF [{}] >> >> >>",
            refs.join(" "),
            refs.join(" "),
            off.join(" ")
        )
        .into_bytes(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>".to_vec(),
    ];
    for (name, _) in groups {
        objects.push(format!("<< /Type /OCG /Name ({name}) >>").into_bytes());
    }

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

fn layers_of(bytes: &[u8], label: &str) -> (u32, bool, String) {
    let dir = scratch(&format!("layers-{label}"));
    let path = dir.join("doc.pdf");
    std::fs::write(&path, bytes).expect("write document");

    let mut coord = DocumentCoordinator::open(&worker_path(), &path).expect("open document");
    let result = coord.get_layers().expect("query layers");
    let _ = coord.close();
    let _ = std::fs::remove_dir_all(&dir);

    (result.count, result.flag, result.data)
}

#[test]
fn a_document_with_layers_reports_them_by_name() {
    let document = document_with_layers(&[("Floor plan", true), ("Wiring", false)]);
    let (count, has_layers, data) = layers_of(&document, "present");

    assert!(has_layers, "a document with two OCGs reported none");
    assert_eq!(count, 2, "wrong number of groups: {data:?}");
    assert!(
        data.contains("Floor plan\ttrue"),
        "the visible group's name and state must survive the trip: {data:?}"
    );
    assert!(
        data.contains("Wiring\tfalse"),
        "a group the default configuration turns off must arrive off: {data:?}"
    );
}

#[test]
fn a_document_without_layers_still_reports_none() {
    // The other half of the claim: the old code was right about this case and
    // wrong about every other, so a test that only checks documents with
    // layers cannot tell the difference.
    let document = document_with_layers(&[]);
    let (count, has_layers, data) = layers_of(&document, "absent");

    assert!(!has_layers);
    assert_eq!(count, 0);
    assert!(data.trim().is_empty(), "unexpected layer data: {data:?}");
}
