//! Appearance content must fall inside the BBox that clips it. [FR-ANNOT-2]
//!
//! A form XObject's content is clipped to its `/BBox`. The annotation
//! generators draw at `rect.x, rect.y` — page coordinates — while the BBox is
//! `[0 0 width height]`, so an annotation anywhere but the page origin drew
//! entirely outside its own box: a conforming reader shows nothing at all.
//!
//! This repository's canvas draws annotations itself in Qt and never
//! rasterizes these streams, so no renderer here can observe the difference.
//! The artefact under test is therefore the emitted PDF — where the drawing
//! operators land relative to the box that clips them, and whether the font the
//! text operators name is defined. That is what another reader looks at.

use pdf_model::annotation::{Annotation, AnnotationType, Color, Point, Rect, TextMarkupKind};
use pdf_model::appearance::build_annotation_pdf_objects;

/// The `/BBox` numbers of an appearance stream object.
fn bbox(ap: &str) -> [f32; 4] {
    let at = ap.find("/BBox [").expect("appearance has a BBox") + "/BBox [".len();
    let end = at + ap[at..].find(']').expect("unclosed BBox");
    let numbers: Vec<f32> = ap[at..end]
        .split_whitespace()
        .map(|n| n.parse().expect("BBox holds numbers"))
        .collect();
    [numbers[0], numbers[1], numbers[2], numbers[3]]
}

/// Every `x y w h re` rectangle in the stream, in the form's own space.
///
/// Generators disagree about whether they translate first, so the running
/// `cm` translation is accumulated as the stream is walked. Only translations
/// appear in these streams; a scale or rotation would need real interpretation
/// and this asserts that none is there.
fn rectangles(ap: &str) -> Vec<[f32; 4]> {
    let stream = &ap[ap.find("stream").expect("has a stream")..];
    let (mut dx, mut dy) = (0.0f32, 0.0f32);
    let mut found = Vec::new();

    for line in stream.lines() {
        let line = line.trim();

        if let Some(head) = line.strip_suffix(" cm") {
            let operands: Vec<f32> = head
                .split_whitespace()
                .filter_map(|token| token.parse().ok())
                .collect();
            assert_eq!(operands.len(), 6, "a cm takes six operands: {line}");
            assert_eq!(
                (operands[0], operands[1], operands[2], operands[3]),
                (1.0, 0.0, 0.0, 1.0),
                "only translations are expected in an appearance stream: {line}"
            );
            dx += operands[4];
            dy += operands[5];
            continue;
        }

        let Some(head) = line
            .strip_suffix(" re f")
            .or_else(|| line.strip_suffix(" re S"))
            .or_else(|| line.strip_suffix(" re"))
        else {
            continue;
        };
        let numbers: Vec<f32> = head
            .split_whitespace()
            .filter_map(|token| token.parse().ok())
            .collect();
        if numbers.len() == 4 {
            found.push([numbers[0] + dx, numbers[1] + dy, numbers[2], numbers[3]]);
        }
    }
    found
}

fn assert_inside_bbox(ap: &str, label: &str) {
    let [bx, by, bw, bh] = bbox(ap);
    let drawn = rectangles(ap);
    assert!(
        !drawn.is_empty(),
        "{label}: the appearance draws no rectangle at all:\n{ap}"
    );
    for [x, y, w, h] in drawn {
        assert!(
            x >= bx - 0.01 && y >= by - 0.01 && x + w <= bw + 0.01 && y + h <= bh + 0.01,
            "{label}: a rectangle at ({x}, {y}) sized {w}x{h} falls outside BBox \
             [{bx} {by} {bw} {bh}] and is clipped away:\n{ap}"
        );
    }
}

/// One annotation of each rectangle-drawing type, placed away from the origin —
/// which is where the defect hid, because every fixture used (0, 0).
fn rect_drawing_types() -> Vec<(&'static str, AnnotationType)> {
    vec![
        (
            "highlight",
            AnnotationType::TextMarkup(TextMarkupKind::Highlight),
        ),
        (
            "underline",
            AnnotationType::TextMarkup(TextMarkupKind::Underline),
        ),
        (
            "strikeout",
            AnnotationType::TextMarkup(TextMarkupKind::Strikeout),
        ),
        ("sticky note", AnnotationType::StickyNote),
        ("free text", AnnotationType::FreeText),
        ("rectangle", AnnotationType::Rectangle),
        ("stamp", AnnotationType::Stamp),
        ("redaction", AnnotationType::Redaction),
    ]
}

fn annotation_at(kind: AnnotationType, x: f32, y: f32) -> Annotation {
    let mut annotation = Annotation::new(1, 0, kind, Rect::new(x, y, 80.0, 24.0))
        .with_contents("visible?")
        .with_author("tester");
    annotation.properties.color = Color::new(1.0, 1.0, 0.0, 0.4);
    annotation
}

#[test]
fn every_appearance_draws_inside_the_box_that_clips_it() {
    for (label, kind) in rect_drawing_types() {
        let mut annotation = annotation_at(kind, 200.0, 300.0);
        let objects = build_annotation_pdf_objects(&mut annotation, 40, 41);
        assert_inside_bbox(&String::from_utf8_lossy(&objects.ap_bytes), label);
    }
}

#[test]
fn an_annotation_at_the_page_origin_is_unaffected() {
    for (label, kind) in rect_drawing_types() {
        let mut annotation = annotation_at(kind, 0.0, 0.0);
        let objects = build_annotation_pdf_objects(&mut annotation, 40, 41);
        assert_inside_bbox(&String::from_utf8_lossy(&objects.ap_bytes), label);
    }
}

#[test]
fn ink_strokes_land_inside_the_box_too() {
    // Ink points are absolute page coordinates taken from the user's stroke,
    // so they need the same translation the shapes do.
    let mut annotation = Annotation::new(
        7,
        0,
        AnnotationType::Ink,
        Rect::new(150.0, 250.0, 60.0, 40.0),
    );
    annotation.ink_points = vec![vec![
        Point { x: 155.0, y: 255.0 },
        Point { x: 195.0, y: 285.0 },
    ]];

    let objects = build_annotation_pdf_objects(&mut annotation, 42, 43);
    let ap = String::from_utf8_lossy(&objects.ap_bytes).to_string();
    let stream = &ap[ap.find("stream").expect("has a stream")..];

    let (mut dx, mut dy) = (0.0f32, 0.0f32);
    let mut points = Vec::new();
    for line in stream.lines() {
        let line = line.trim();
        if let Some(head) = line.strip_suffix(" cm") {
            let operands: Vec<f32> = head
                .split_whitespace()
                .filter_map(|t| t.parse().ok())
                .collect();
            dx += operands[4];
            dy += operands[5];
        } else if let Some(head) = line.strip_suffix(" m").or_else(|| line.strip_suffix(" l")) {
            let operands: Vec<f32> = head
                .split_whitespace()
                .filter_map(|t| t.parse().ok())
                .collect();
            if operands.len() == 2 {
                points.push((operands[0] + dx, operands[1] + dy));
            }
        }
    }

    assert!(!points.is_empty(), "the ink appearance draws no path:\n{ap}");
    let [_, _, bw, bh] = bbox(&ap);
    for (x, y) in points {
        assert!(
            x >= -0.01 && y >= -0.01 && x <= bw + 0.01 && y <= bh + 0.01,
            "an ink point at ({x}, {y}) is outside BBox [0 0 {bw} {bh}]:\n{ap}"
        );
    }
}

#[test]
fn text_bearing_appearances_define_the_font_they_name() {
    // The streams say `/F1 10 Tf`. A form XObject that declares no resources
    // leaves /F1 undefined: a reader may fall back to the page's resources,
    // which need not contain it, or draw nothing.
    let mut annotation = annotation_at(AnnotationType::FreeText, 120.0, 400.0);
    let objects = build_annotation_pdf_objects(&mut annotation, 50, 51);
    let ap = String::from_utf8_lossy(&objects.ap_bytes).to_string();

    assert!(ap.contains("Tf"), "this fixture is meant to draw text:\n{ap}");
    assert!(
        ap.contains("/Resources") && ap.contains("/F1 <<") && ap.contains("/BaseFont /Helvetica"),
        "the appearance names /F1 but defines no font:\n{ap}"
    );
}

#[test]
fn an_appearance_with_no_text_carries_no_font_resource() {
    let mut annotation = annotation_at(
        AnnotationType::TextMarkup(TextMarkupKind::Highlight),
        200.0,
        300.0,
    );
    let objects = build_annotation_pdf_objects(&mut annotation, 60, 61);
    let ap = String::from_utf8_lossy(&objects.ap_bytes).to_string();

    assert!(!ap.contains("Tf"), "fixture unexpectedly draws text:\n{ap}");
    assert!(
        !ap.contains("/Resources"),
        "a shape-only appearance needs no font resource:\n{ap}"
    );
}

#[test]
fn the_translation_is_balanced_by_a_restore() {
    // `q ... cm` without the matching `Q` leaves the transform applied to
    // whatever a reader draws next.
    let mut annotation = annotation_at(
        AnnotationType::TextMarkup(TextMarkupKind::Highlight),
        200.0,
        300.0,
    );
    let objects = build_annotation_pdf_objects(&mut annotation, 70, 71);
    let ap = String::from_utf8_lossy(&objects.ap_bytes).to_string();
    let stream = &ap[ap.find("stream").expect("has a stream")..];

    assert_eq!(stream.matches(" q\n").count() + usize::from(stream.contains("\nq ")), 1);
    assert!(stream.contains("\nQ\n"), "the transform is never restored:\n{ap}");
}
