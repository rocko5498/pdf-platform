//! Appearance differences a text diff cannot see. [FR-CMP-1]
//!
//! FR-CMP-1 requires comparison of "text content **and** visual/page
//! appearance". The failure this guards against is specific and easy to ship:
//! two documents whose extracted text is byte-identical, where one has a black
//! box drawn over a clause. A text-only comparison reports them as identical,
//! which for a redaction review is the worst possible answer.

use std::io::Write;
use std::path::PathBuf;

use coordinator::document::DocumentCoordinator;
use coordinator::visual_compare::{compare_pages, DEFAULT_CHANNEL_TOLERANCE};

fn worker_path() -> PathBuf {
    let exe = std::env::current_exe().expect("current exe");
    let mut dir = exe.parent().expect("exe dir").to_path_buf();
    if dir.ends_with("deps") {
        dir.pop();
    }
    dir.join(if cfg!(windows) { "worker.exe" } else { "worker" })
}

/// Build a one-page PDF around `content`, computing a correct xref.
fn pdf_with_content(content: &str) -> Vec<u8> {
    let content = content.as_bytes();
    let objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
           /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>"
            .to_vec(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        [
            format!("<< /Length {} >>\nstream\n", content.len()).as_bytes(),
            content,
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

fn write_temp(name: &str, bytes: &[u8]) -> PathBuf {
    let path = std::env::temp_dir().join(format!("pdf-platform-visual-{name}.pdf"));
    let mut file = std::fs::File::create(&path).expect("create fixture");
    file.write_all(bytes).expect("write fixture");
    path
}

const TEXT: &str = "BT\n/F1 12 Tf\n72 720 Td\n(confidential terms apply) Tj\nET\n";

#[test]
fn a_box_drawn_over_a_clause_is_visible_even_though_the_text_is_identical() {
    let worker = worker_path();
    assert!(worker.is_file(), "worker binary missing at {}", worker.display());

    let plain = write_temp("plain", &pdf_with_content(TEXT));
    // Same text, plus a filled black rectangle over the clause.
    let boxed = write_temp(
        "boxed",
        &pdf_with_content(&format!("{TEXT}0 0 0 rg\n60 700 200 40 re\nf\n")),
    );

    let mut before = DocumentCoordinator::open(&worker, &plain).expect("open plain");
    let mut after = DocumentCoordinator::open(&worker, &boxed).expect("open boxed");

    // The premise: extracted text is identical, so a text diff sees nothing.
    let lines_of = |model: &engine_api::extract::PageTextModel| -> Vec<String> {
        model.lines.iter().map(|line| line.text.clone()).collect()
    };
    let text_before = lines_of(before.get_page_text(0).expect("text before"));
    let text_after = lines_of(after.get_page_text(0).expect("text after"));
    assert_eq!(
        text_before, text_after,
        "fixtures must differ only in appearance for this test to mean anything"
    );

    let rendered_before = before.render_page(0, 0.5).expect("render before");
    let rendered_after = after.render_page(0, 0.5).expect("render after");
    let diff = compare_pages(&rendered_before, &rendered_after, DEFAULT_CHANNEL_TOLERANCE)
        .expect("compare pages");

    assert!(
        !diff.is_identical(),
        "a black box drawn over a clause must be reported as an appearance change"
    );
    assert!(
        diff.changed_fraction() > 0.001,
        "expected a measurable region to differ, got {:.4}%",
        diff.changed_fraction() * 100.0
    );
    assert_eq!(diff.max_channel_delta, 255, "black over white is a full-range delta");

    let _ = std::fs::remove_file(&plain);
    let _ = std::fs::remove_file(&boxed);
}

#[test]
fn two_renders_of_the_same_document_are_reported_unchanged() {
    // The other half: the comparison must not cry wolf on rendering noise, or
    // every review would drown in false positives.
    let worker = worker_path();
    assert!(worker.is_file(), "worker binary missing at {}", worker.display());

    let path = write_temp("stable", &pdf_with_content(TEXT));
    let mut first = DocumentCoordinator::open(&worker, &path).expect("open first");
    let mut second = DocumentCoordinator::open(&worker, &path).expect("open second");

    let a = first.render_page(0, 0.5).expect("render a");
    let b = second.render_page(0, 0.5).expect("render b");
    let diff = compare_pages(&a, &b, DEFAULT_CHANNEL_TOLERANCE).expect("compare pages");

    assert!(diff.is_identical(), "identical input rendered differently: {diff:?}");

    let _ = std::fs::remove_file(&path);
}
