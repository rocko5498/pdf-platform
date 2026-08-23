//! Encryption must actually keep content out, and the password must let it in.
//! [FR-VIEW-6, FR-SEC-1, PRIN-1]
//!
//! `pipeline_e2e`'s encrypted test opens the document without a password and
//! then writes `let _ = outline;` — it asserts nothing at all about that case.
//! If encryption were bypassed entirely and the text were readable to anyone,
//! it would still pass. With the password it checks that a page count comes
//! back and that structure queries do not error, never that the decrypted text
//! is the text that was encrypted.
//!
//! So this asserts both halves: no password means no content, and the right
//! password means the *original* content.

use std::path::{Path, PathBuf};

use coordinator::session::WorkerSession;
use pdf_model::assembly_ops::qpdf_available;

fn worker_path() -> PathBuf {
    let exe = std::env::current_exe().expect("current exe");
    let mut dir = exe.parent().expect("exe dir").to_path_buf();
    if dir.ends_with("deps") {
        dir.pop();
    }
    dir.join(if cfg!(windows) { "worker.exe" } else { "worker" })
}

fn qpdf_required() -> bool {
    std::env::var_os("PDF_PLATFORM_REQUIRE_QPDF").is_some()
}

/// The word the encrypted document contains, and nothing else does.
const SECRET: &str = "SECRETMARK";

fn plaintext_pdf() -> Vec<u8> {
    let content = format!("BT\n/F1 24 Tf\n72 700 Td\n({SECRET}) Tj\nET\n");
    let objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
           /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>"
            .to_vec(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        [
            format!("<< /Length {} >>\nstream\n", content.len()).as_bytes(),
            content.as_bytes(),
            b"endstream",
        ]
        .concat(),
    ];

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

fn encrypt(plain: &Path, out: &Path, user_password: &str) {
    let status = std::process::Command::new("qpdf")
        .args([
            "--encrypt",
            user_password,
            "owner-secret-test",
            "256",
            "--",
            plain.to_str().expect("utf8 path"),
            out.to_str().expect("utf8 path"),
        ])
        .status()
        .expect("run qpdf encrypt");
    assert!(status.success(), "qpdf encrypt failed");
}

fn page_text(session: &mut WorkerSession) -> String {
    match session.extract_page(0) {
        Ok(model) => model.lines.iter().map(|line| line.text.as_str()).collect(),
        Err(_) => String::new(),
    }
}

#[test]
fn an_encrypted_document_yields_nothing_without_its_password() {
    if !qpdf_available() {
        assert!(!qpdf_required(), "PDF_PLATFORM_REQUIRE_QPDF is set but qpdf is missing");
        eprintln!("skip: qpdf not on PATH");
        return;
    }

    let dir = std::env::temp_dir();
    let plain = dir.join("pdf-platform-enc-plain.pdf");
    let encrypted = dir.join("pdf-platform-enc-locked.pdf");
    std::fs::write(&plain, plaintext_pdf()).expect("write plaintext");
    encrypt(&plain, &encrypted, "secret-user");

    // Sanity: the plaintext really does contain the word, so a failure below
    // means encryption held rather than the fixture being empty.
    let brokered = coordinator::broker::open_read_only(&plain).expect("broker plain");
    let mut session =
        WorkerSession::spawn_with_document(&worker_path(), brokered).expect("spawn plain");
    let _ = session.inspect();
    assert!(
        page_text(&mut session).contains(SECRET),
        "the fixture must contain the secret before encryption, or this test proves nothing"
    );
    drop(session);

    // No password: the content must not come out.
    let brokered = coordinator::broker::open_read_only(&encrypted).expect("broker encrypted");
    let mut session =
        WorkerSession::spawn_with_document(&worker_path(), brokered).expect("spawn encrypted");
    let _ = session.inspect();
    let leaked = page_text(&mut session);
    drop(session);

    assert!(
        !leaked.contains(SECRET),
        "encrypted content was readable without the password: {leaked:?}"
    );

    let _ = std::fs::remove_file(&plain);
    let _ = std::fs::remove_file(&encrypted);
}

#[test]
fn the_right_password_returns_the_original_text() {
    if !qpdf_available() {
        assert!(!qpdf_required(), "PDF_PLATFORM_REQUIRE_QPDF is set but qpdf is missing");
        eprintln!("skip: qpdf not on PATH");
        return;
    }

    let dir = std::env::temp_dir();
    let plain = dir.join("pdf-platform-enc-plain2.pdf");
    let encrypted = dir.join("pdf-platform-enc-locked2.pdf");
    std::fs::write(&plain, plaintext_pdf()).expect("write plaintext");
    encrypt(&plain, &encrypted, "secret-user");

    let brokered = coordinator::broker::open_read_only(&encrypted).expect("broker encrypted");
    let mut session = WorkerSession::spawn_with_document_password(
        &worker_path(),
        brokered,
        Some("secret-user"),
    )
    .expect("spawn with password");
    let summary = session.inspect().expect("inspect with password");
    assert!(summary.page_count >= 1);

    // The point: decryption must return the text that was encrypted, not merely
    // succeed at opening something.
    let text = page_text(&mut session);
    drop(session);
    assert!(
        text.contains(SECRET),
        "decrypted page did not contain the original text: {text:?}"
    );

    let _ = std::fs::remove_file(&plain);
    let _ = std::fs::remove_file(&encrypted);
}
