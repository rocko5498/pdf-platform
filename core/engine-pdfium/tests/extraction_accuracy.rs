//! Extraction accuracy against a real PDF, parsed by the real engine.
//!
//! `core/text-extract/tests/extraction_correctness.rs` drives a `StaticEngine`
//! over hand-written `PageTextModel` values: no PDF is parsed and no engine
//! runs, which `docs/release-gates.md` says in as many words. It proves
//! normalization and search behave *given* a text model — real, but it cannot
//! discharge MET-FEAT-4 or T-2, which measure extraction against documents.
//!
//! This is the first datapoint that can: a fixture whose text is known because
//! the content stream was written by hand, opened through PDFium, with the
//! extracted string compared to it exactly.
//!
//! Scope, stated rather than implied: one page, base-14 Helvetica, ASCII Latin.
//! Ligatures, soft hyphens, CJK, RTL and ToUnicode pathologies are not covered
//! here — those need embedded-font fixtures and are the corpus work MET-FEAT-4
//! still wants. [ADR-019, ADR-022 T-2, MET-FEAT-4, PRIN-6, GR-8]

use std::path::PathBuf;

use engine_api::extract::Extract;
use engine_pdfium::PdfiumEngine;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tools")
        .join("corpus-diff")
        .join("fixtures")
        .join(name)
}

/// The exact strings drawn by `text-latin.pdf`'s content stream.
const EXPECTED_LINES: [&str; 2] = ["Hello extraction", "second line here"];

#[test]
fn latin_text_extracts_exactly_as_drawn() {
    let path = fixture("text-latin.pdf");
    assert!(
        path.is_file(),
        "fixture missing at {} — extraction accuracy cannot be measured without it",
        path.display()
    );

    // A missing engine is a failure, not a skip: PDFium is provisioned before
    // the build by `tools/provision_engine.py`. [ADR-038]
    let engine = PdfiumEngine::from_file(&path, None)
        .unwrap_or_else(|error| panic!("open {}: {error}", path.display()));

    let model = engine.extract_page(0).expect("extract page 0");

    let extracted: Vec<String> = model
        .lines
        .iter()
        .map(|line| line.text.trim().to_owned())
        .filter(|text| !text.is_empty())
        .collect();

    assert_eq!(
        extracted,
        EXPECTED_LINES.map(str::to_owned).to_vec(),
        "extracted text does not match the content stream that produced it"
    );

    // Character count must describe the text actually returned, not a guess:
    // downstream search and OCR-skip decisions read it. [FR-SRCH-5]
    let counted: u32 = model.lines.iter().map(|line| line.text.chars().count() as u32).sum();
    assert_eq!(model.char_count, counted, "char_count disagrees with the lines");

    // Plain Helvetica text has no private-use glyphs, so the reliability flag
    // must say so. It was hard-coded `true` until 2026-07-27; this pins the
    // honest answer for a page where the answer is knowable. [ADR-019 §4]
    assert!(model.reliable, "a base-14 Latin page must extract reliably");
}

#[test]
fn line_geometry_is_inside_the_page_box() {
    // Geometry feeds hit-testing, selection and OCR block placement. A line
    // outside the MediaBox means the coordinate space is wrong somewhere, which
    // no string comparison would catch. [FR-SRCH-2, SDS §3.3]
    let path = fixture("text-latin.pdf");
    let engine = PdfiumEngine::from_file(&path, None).expect("open fixture");
    let model = engine.extract_page(0).expect("extract page 0");

    const PAGE_WIDTH: f32 = 612.0;
    const PAGE_HEIGHT: f32 = 792.0;
    for line in &model.lines {
        assert!(line.x >= 0.0 && line.x <= PAGE_WIDTH, "x out of page: {line:?}");
        assert!(line.y >= 0.0 && line.y <= PAGE_HEIGHT, "y out of page: {line:?}");
        assert!(line.width > 0.0, "zero-width line: {line:?}");
        assert!(line.height > 0.0, "zero-height line: {line:?}");
    }
}

#[test]
fn concurrent_extraction_returns_the_same_text_every_time() {
    // `PdfiumEngine` carries `unsafe impl Send + Sync`, so the type system
    // permits exactly this. PDFium is not thread-safe, and before the engine
    // serialized its calls two threads in one process were enough to corrupt
    // each other: Windows returned "Hello extraction" as "Helo xtracion" and
    // Linux aborted with `free(): invalid pointer`. Silent text corruption is
    // worse than a crash, because nothing downstream can tell. [CR-4, PRIN-1]
    use std::sync::Arc;

    let path = fixture("text-latin.pdf");
    let engine = Arc::new(PdfiumEngine::from_file(&path, None).expect("open fixture"));

    let handles: Vec<_> = (0..8)
        .map(|_| {
            let engine = Arc::clone(&engine);
            std::thread::spawn(move || {
                let model = engine.extract_page(0).expect("extract page 0");
                model
                    .lines
                    .iter()
                    .map(|line| line.text.trim().to_owned())
                    .filter(|text| !text.is_empty())
                    .collect::<Vec<_>>()
            })
        })
        .collect();

    for handle in handles {
        let extracted = handle.join().expect("extraction thread panicked");
        assert_eq!(
            extracted,
            EXPECTED_LINES.map(str::to_owned).to_vec(),
            "concurrent extraction returned different text"
        );
    }
}
