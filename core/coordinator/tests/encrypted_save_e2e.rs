//! Saving changes into an encrypted document. [FR-SAVE, FR-SEC-1, PRIN-1]
//!
//! An encrypted PDF encrypts its strings and streams. The incremental writer
//! appends **plaintext** objects, and the trailer it now carries forward keeps
//! `/Encrypt` — so every conforming reader would decrypt those new objects and
//! get noise. A stamped or filled encrypted document would come back corrupt in
//! someone else's application, silently, because our own reader never decrypts
//! anything (PDFium does that for us) and would show it as fine.
//!
//! Encrypting on write is a crypto change and needs a human owner (`AI-6`).
//! Refusing, with a reason the user can act on, is what `PRIN-1` and `GR-8` ask
//! for in the meantime.

mod common;

use std::io::Write as _;

use common::{numbered_pdf, scratch, worker_path};
use coordinator::document::DocumentCoordinator;

/// A document whose trailer declares `/Encrypt`. The encryption dictionary is
/// well-formed enough to be found; no content is actually encrypted, because
/// what is under test is the writer's refusal, not the cipher.
fn declares_encryption() -> Vec<u8> {
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
        b"<< /Filter /Standard /V 2 /R 3 /Length 128 /P -3904 >>".to_vec(),
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
        "trailer\n<< /Size {} /Root 1 0 R /Encrypt 6 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        objects.len() + 1
    )
    .unwrap();
    bytes
}

#[test]
fn saving_changes_into_an_encrypted_document_is_refused_with_a_reason() {
    let dir = scratch("encrypted-save");
    let source = dir.join("encrypted.pdf");
    std::fs::write(&source, declares_encryption()).expect("write source");

    let mut coord = DocumentCoordinator::open(&worker_path(), &source).expect("open");
    coord
        .apply_stamp(&pdf_model::stamp::Stamp {
            text: "REVIEWED".into(),
            ..pdf_model::stamp::Stamp::default()
        })
        .expect("staging the edit is fine; writing it is not");

    let saved = dir.join("saved.pdf");
    let error = coord
        .save_incremental(&saved)
        .expect_err("writing plaintext objects into an encrypted document must be refused");
    let message = error.to_string();

    assert!(
        message.contains("encrypted"),
        "the refusal must name the reason: {message}"
    );
    assert!(
        message.contains("not implemented") || message.contains("remove the encryption"),
        "the refusal must tell the user what to do: {message}"
    );
    assert!(
        !saved.exists() || std::fs::metadata(&saved).map(|m| m.len()).unwrap_or(0) == 0,
        "a refused save must not leave a corrupt file behind"
    );

    let _ = coord.close();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_unencrypted_document_still_saves() {
    // The other half: the refusal must not catch everything.
    let dir = scratch("encrypted-save-control");
    let source = dir.join("plain.pdf");
    std::fs::write(&source, numbered_pdf(1)).expect("write source");

    let mut coord = DocumentCoordinator::open(&worker_path(), &source).expect("open");
    coord
        .apply_stamp(&pdf_model::stamp::Stamp {
            text: "REVIEWED".into(),
            ..pdf_model::stamp::Stamp::default()
        })
        .expect("stamp");
    let saved = dir.join("saved.pdf");
    coord.save_incremental(&saved).expect("an unencrypted save must go through");
    let _ = coord.close();

    let bytes = std::fs::read(&saved).expect("read saved");
    assert!(bytes.windows(b"REVIEWED".len()).any(|w| w == b"REVIEWED"));

    let _ = std::fs::remove_dir_all(&dir);
}
