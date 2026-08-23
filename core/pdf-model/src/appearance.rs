//! Appearance stream generator for annotations. [FR-ANNOT-2, SDS §2.2.9]
//!
//! Generates PDF content streams that visually represent annotations,
//! so they render correctly in other conformant readers. The Platform
//! MUST NOT write appearance-less annotations (FR-ANNOT-2).
//!
//! Appearance streams are PDF content streams that define how an annotation
//! looks: paths, text, colors, and transforms. Each annotation type has
//! its own generation strategy.
//!
//! **Forms (M5):** widget annotations for form fields follow the same honesty
//! rule after forms JS calculations update values — regenerate `/AP` before
//! save via [`generate_widget_appearance`] / [`build_widget_pdf_objects`].
//! [ADR-017, FR-FORM-1, FR-JS]

use std::io::Write;

use crate::annotation::{
    Annotation, AnnotationType, BorderStyle, Color,
    Rect, TextMarkupKind,
};
use crate::form::{FieldType, FieldValue, FormField};

/// Generate an appearance stream for an annotation. [FR-ANNOT-2]
///
/// Returns the PDF content stream bytes (inside a stream dictionary).
/// The caller wraps this in the appropriate annotation dictionary entry.
pub fn generate_appearance(annotation: &Annotation) -> Vec<u8> {
    match annotation.annotation_type {
        AnnotationType::TextMarkup(kind) => {
            generate_text_markup_appearance(&annotation.rect, kind, &annotation.properties.color)
        }
        AnnotationType::StickyNote => {
            generate_note_appearance(&annotation.rect, &annotation.properties.color)
        }
        AnnotationType::FreeText | AnnotationType::Callout => {
            generate_freetext_appearance(
                &annotation.rect,
                &annotation.properties.contents,
                &annotation.properties.color,
                &annotation.properties.border,
            )
        }
        AnnotationType::Ink => {
            generate_ink_appearance(&annotation.ink_points, &annotation.properties.color, &annotation.properties.border)
        }
        AnnotationType::Line | AnnotationType::Arrow => {
            generate_line_appearance(
                &annotation.shape_points,
                &annotation.properties.color,
                &annotation.properties.border,
                matches!(annotation.annotation_type, AnnotationType::Arrow),
            )
        }
        AnnotationType::Rectangle => {
            generate_rectangle_appearance(&annotation.rect, &annotation.properties.color, &annotation.properties.border)
        }
        AnnotationType::Ellipse => {
            generate_ellipse_appearance(&annotation.rect, &annotation.properties.color, &annotation.properties.border)
        }
        AnnotationType::Polygon | AnnotationType::Polyline => {
            generate_polygon_appearance(&annotation.shape_points, &annotation.properties.color, &annotation.properties.border,
                matches!(annotation.annotation_type, AnnotationType::Polygon))
        }
        AnnotationType::Stamp => {
            generate_stamp_appearance(&annotation.rect, &annotation.properties.contents)
        }
        AnnotationType::Redaction => {
            generate_redaction_appearance(&annotation.rect, &annotation.properties.color)
        }
    }
}

/// PDF object pair (annotation dict + appearance stream) for the CoW overlay.
/// [FR-ANNOT-2, SDS §2.9]
#[derive(Debug, Clone)]
pub struct AnnotationPdfObjects {
    /// 1-based object number for the annotation dictionary.
    pub annot_obj_num: u32,
    /// 1-based object number for the appearance stream.
    pub ap_obj_num: u32,
    /// Serialized annotation dictionary object bytes (`N 0 obj ... endobj`).
    pub annot_bytes: Vec<u8>,
    /// Serialized appearance stream object bytes.
    pub ap_bytes: Vec<u8>,
}

/// Build PDF objects for an annotation with a guaranteed appearance stream.
///
/// Always writes `/AP << /N N 0 R >>` so other readers render the annotation
/// portably (FR-ANNOT-2). Generates appearance if the annotation lacks one.
pub fn build_annotation_pdf_objects(
    annotation: &mut Annotation,
    annot_obj_num: u32,
    ap_obj_num: u32,
) -> AnnotationPdfObjects {
    annotation.ensure_appearance();
    let content = annotation
        .appearance
        .as_ref()
        .expect("ensure_appearance always sets appearance")
        .clone();

    let ap_bytes = serialize_appearance_stream_object(ap_obj_num, &annotation.rect, &content);
    let annot_bytes = serialize_annotation_dict_object(annotation, annot_obj_num, ap_obj_num);

    AnnotationPdfObjects {
        annot_obj_num,
        ap_obj_num,
        annot_bytes,
        ap_bytes,
    }
}

/// Serialize an appearance XObject stream. [FR-ANNOT-2]
fn serialize_appearance_stream_object(obj_num: u32, rect: &Rect, content: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let _ = writeln!(out, "{obj_num} 0 obj");
    let _ = writeln!(
        out,
        "<< /Type /XObject /Subtype /Form /BBox [0 0 {} {}] /Length {} >>",
        rect.width,
        rect.height,
        content.len()
    );
    let _ = writeln!(out, "stream");
    out.extend_from_slice(content);
    if !content.ends_with(b"\n") {
        out.push(b'\n');
    }
    let _ = writeln!(out, "endstream");
    let _ = writeln!(out, "endobj");
    out
}

/// Serialize an annotation dictionary with `/AP` pointing at the appearance stream.
fn serialize_annotation_dict_object(
    annotation: &Annotation,
    annot_obj_num: u32,
    ap_obj_num: u32,
) -> Vec<u8> {
    let subtype = annotation
        .pdf_subtype_str()
        .unwrap_or_else(|| annotation.pdf_type_str());
    let rect = &annotation.rect;
    let c = &annotation.properties.color;
    let contents = escape_pdf_string(&annotation.properties.contents);
    let author = escape_pdf_string(&annotation.properties.author);

    let mut out = Vec::new();
    let _ = writeln!(out, "{annot_obj_num} 0 obj");
    let _ = write!(out, "<< /Type /Annot /Subtype /{subtype} ");
    let _ = write!(
        out,
        "/Rect [{} {} {} {}] ",
        rect.x,
        rect.y,
        rect.x + rect.width,
        rect.y + rect.height
    );
    let _ = write!(out, "/C [{} {} {}] ", c.r, c.g, c.b);
    let _ = write!(out, "/CA {} ", c.a);
    if !contents.is_empty() {
        let _ = write!(out, "/Contents ({contents}) ");
    }
    if !author.is_empty() {
        let _ = write!(out, "/T ({author}) ");
    }
    // Always-written appearance: never emit appearance-less annotations. [FR-ANNOT-2]
    let _ = write!(out, "/AP << /N {ap_obj_num} 0 R >> ");
    let _ = writeln!(out, "/F 4 >>");
    let _ = writeln!(out, "endobj");
    out
}

// ---------------------------------------------------------------------------
// Per-type appearance generators
// ---------------------------------------------------------------------------

/// Text markup: colored rectangle overlay. [FR-ANNOT-1]
fn generate_text_markup_appearance(
    rect: &Rect,
    kind: TextMarkupKind,
    color: &Color,
) -> Vec<u8> {
    let mut buf = Vec::new();

    // PDF content stream: draw a filled semi-transparent rectangle.
    let c = color;
    write!(&mut buf, "{:.3} {:.3} {:.3} rg\n", c.r, c.g, c.b).unwrap();
    // The next line used to be `{alpha} g`, described as "alpha via gray".
    // `g` sets the *grey fill colour*, so it overwrote the RGB fill that had
    // just been set: a yellow highlight at alpha 0.4 painted 40% grey. Opacity
    // is carried by the annotation's `/CA`, written in the annotation
    // dictionary above, which is where PDF puts it. [FR-ANNOT-1, PRIN-1]

    // Fill the rect.
    write!(&mut buf, "{:.1} {:.1} {:.1} {:.1} re f\n",
        rect.x, rect.y, rect.width, rect.height).unwrap();

    // For strikeout, draw a line through the middle.
    if kind == TextMarkupKind::Strikeout {
        let mid_y = rect.y + rect.height / 2.0;
        write!(&mut buf, "{:.3} {:.3} {:.3} rg\n", c.r, c.g, c.b).unwrap();
        write!(&mut buf, "1 w\n").unwrap(); // line width
        write!(&mut buf, "{:.1} {:.1} m {:.1} {:.1} l S\n",
            rect.x, mid_y, rect.x + rect.width, mid_y).unwrap();
    }

    // For underline, draw a line at the bottom.
    if kind == TextMarkupKind::Underline {
        let bot_y = rect.y + 2.0;
        write!(&mut buf, "{:.3} {:.3} {:.3} rg\n", c.r, c.g, c.b).unwrap();
        write!(&mut buf, "1 w\n").unwrap();
        write!(&mut buf, "{:.1} {:.1} m {:.1} {:.1} l S\n",
            rect.x, bot_y, rect.x + rect.width, bot_y).unwrap();
    }

    // For squiggly, draw a wavy line at the bottom.
    if kind == TextMarkupKind::Squiggly {
        let bot_y = rect.y + 2.0;
        write!(&mut buf, "{:.3} {:.3} {:.3} rg\n", c.r, c.g, c.b).unwrap();
        write!(&mut buf, "1 w\n").unwrap();
        write!(&mut buf, "{:.1} {:.1} m ", rect.x, bot_y).unwrap();
        let mut x = rect.x + 4.0;
        let mut up = true;
        while x < rect.x + rect.width - 4.0 {
            let y = if up { bot_y + 3.0 } else { bot_y - 1.0 };
            write!(&mut buf, "{:.1} {:.1} l ", x, y).unwrap();
            x += 4.0;
            up = !up;
        }
        write!(&mut buf, "{:.1} {:.1} l S\n", rect.x + rect.width, bot_y).unwrap();
    }

    buf
}

/// Sticky note: small colored icon rectangle. [FR-ANNOT-1]
fn generate_note_appearance(rect: &Rect, color: &Color) -> Vec<u8> {
    let mut buf = Vec::new();
    let c = color;

    // Draw a small filled square (the note icon).
    write!(&mut buf, "{:.3} {:.3} {:.3} rg\n", c.r, c.g, c.b).unwrap();
    write!(&mut buf, "1 0 0 1 {:.1} {:.1} cm\n", rect.x, rect.y).unwrap();
    write!(&mut buf, "0 0 {:.1} {:.1} re f\n", rect.width, rect.height).unwrap();

    // Draw a folded corner triangle.
    let fold = rect.width.min(rect.height) * 0.3;
    write!(&mut buf, "1 g\n").unwrap();
    write!(&mut buf, "{:.1} {:.1} m {:.1} {:.1} l {:.1} {:.1} l f\n",
        rect.x + rect.width - fold, rect.y + rect.height,
        rect.x + rect.width, rect.y + rect.height - fold,
        rect.x + rect.width - fold, rect.y + rect.height - fold).unwrap();

    buf
}

/// Free text: text box with optional border. [FR-ANNOT-1]
fn generate_freetext_appearance(
    rect: &Rect,
    text: &str,
    color: &Color,
    border: &BorderStyle,
) -> Vec<u8> {
    let mut buf = Vec::new();
    let c = color;

    // Fill background (white or light).
    write!(&mut buf, "1 1 1 rg\n").unwrap();
    write!(&mut buf, "{:.1} {:.1} {:.1} {:.1} re f\n",
        rect.x, rect.y, rect.width, rect.height).unwrap();

    // Draw border.
    write!(&mut buf, "{:.3} {:.3} {:.3} rg\n", c.r, c.g, c.b).unwrap();
    write!(&mut buf, "{:.1} w\n", border.width).unwrap();
    write!(&mut buf, "{:.1} {:.1} {:.1} {:.1} re S\n",
        rect.x, rect.y, rect.width, rect.height).unwrap();

    // Draw text (simplified — BT/ET with font).
    if !text.is_empty() {
        write!(&mut buf, "BT\n").unwrap();
        write!(&mut buf, "/F1 10 Tf\n").unwrap();
        write!(&mut buf, "{:.3} {:.3} {:.3} rg\n", c.r, c.g, c.b).unwrap();
        // Escape special PDF characters.
        let escaped = escape_pdf_string(text);
        write!(&mut buf, "{:.1} {:.1} Td\n", rect.x + 4.0, rect.y + rect.height - 14.0).unwrap();
        write!(&mut buf, "({}) Tj\n", escaped).unwrap();
        write!(&mut buf, "ET\n").unwrap();
    }

    buf
}

/// Ink/freehand drawing: stroked path. [FR-ANNOT-1]
fn generate_ink_appearance(
    points: &[Vec<crate::annotation::Point>],
    color: &Color,
    border: &BorderStyle,
) -> Vec<u8> {
    let mut buf = Vec::new();
    let c = color;

    write!(&mut buf, "{:.3} {:.3} {:.3} rg\n", c.r, c.g, c.b).unwrap();
    write!(&mut buf, "{:.1} w\n", border.width).unwrap();
    write!(&mut buf, "1 J\n").unwrap(); // round line cap
    write!(&mut buf, "1 j\n").unwrap(); // round line join

    for stroke in points {
        if stroke.is_empty() {
            continue;
        }
        // Move to first point.
        write!(&mut buf, "{:.1} {:.1} m\n", stroke[0].x, stroke[0].y).unwrap();
        // Line to subsequent points.
        for pt in &stroke[1..] {
            write!(&mut buf, "{:.1} {:.1} l\n", pt.x, pt.y).unwrap();
        }
        write!(&mut buf, "S\n").unwrap(); // stroke
    }

    buf
}

/// Line or arrow: stroked line with optional arrowheads. [FR-ANNOT-1]
fn generate_line_appearance(
    points: &[crate::annotation::Point],
    color: &Color,
    border: &BorderStyle,
    arrow: bool,
) -> Vec<u8> {
    let mut buf = Vec::new();
    let c = color;

    if points.len() < 2 {
        return buf;
    }

    write!(&mut buf, "{:.3} {:.3} {:.3} rg\n", c.r, c.g, c.b).unwrap();
    write!(&mut buf, "{:.1} w\n", border.width).unwrap();

    // Draw the line.
    write!(&mut buf, "{:.1} {:.1} m\n", points[0].x, points[0].y).unwrap();
    write!(&mut buf, "{:.1} {:.1} l S\n", points[1].x, points[1].y).unwrap();

    // Draw arrowhead if requested.
    if arrow && points.len() >= 2 {
        let (x1, y1) = (points[0].x, points[0].y);
        let (x2, y2) = (points[1].x, points[1].y);
        let angle = (y2 - y1).atan2(x2 - x1);
        let head_len = 10.0;
        let head_angle = 0.5; // radians

        let ax = x2 - head_len * (angle - head_angle).cos();
        let ay = y2 - head_len * (angle - head_angle).sin();
        let bx = x2 - head_len * (angle + head_angle).cos();
        let by = y2 - head_len * (angle + head_angle).sin();

        write!(&mut buf, "{:.1} {:.1} m {:.1} {:.1} l {:.1} {:.1} l f\n",
            x2, y2, ax, ay, bx, by).unwrap();
    }

    buf
}

/// Rectangle: stroked/filled rectangle. [FR-ANNOT-1]
fn generate_rectangle_appearance(
    rect: &Rect,
    color: &Color,
    border: &BorderStyle,
) -> Vec<u8> {
    let mut buf = Vec::new();
    let c = color;

    write!(&mut buf, "{:.3} {:.3} {:.3} rg\n", c.r, c.g, c.b).unwrap();
    write!(&mut buf, "{:.1} w\n", border.width).unwrap();

    // Stroke the rectangle.
    write!(&mut buf, "{:.1} {:.1} {:.1} {:.1} re S\n",
        rect.x, rect.y, rect.width, rect.height).unwrap();

    buf
}

/// Ellipse: stroked/filled ellipse. [FR-ANNOT-1]
fn generate_ellipse_appearance(
    rect: &Rect,
    color: &Color,
    border: &BorderStyle,
) -> Vec<u8> {
    let mut buf = Vec::new();
    let c = color;

    write!(&mut buf, "{:.3} {:.3} {:.3} rg\n", c.r, c.g, c.b).unwrap();
    write!(&mut buf, "{:.1} w\n", border.width).unwrap();

    // Approximate ellipse with bezier curves.
    let cx = rect.x + rect.width / 2.0;
    let cy = rect.y + rect.height / 2.0;
    let rx = rect.width / 2.0;
    let ry = rect.height / 2.0;
    let k = 0.5522847498; // magic number for cubic bezier circle approximation

    write!(&mut buf, "{:.1} {:.1} m\n", cx, cy - ry).unwrap();
    write!(&mut buf, "{:.1} {:.1} {:.1} {:.1} {:.1} {:.1} c\n",
        cx + rx * k, cy - ry, cx + rx, cy - ry * k, cx + rx, cy).unwrap();
    write!(&mut buf, "{:.1} {:.1} {:.1} {:.1} {:.1} {:.1} c\n",
        cx + rx, cy + ry * k, cx + rx * k, cy + ry, cx, cy + ry).unwrap();
    write!(&mut buf, "{:.1} {:.1} {:.1} {:.1} {:.1} {:.1} c\n",
        cx - rx * k, cy + ry, cx - rx, cy + ry * k, cx - rx, cy).unwrap();
    write!(&mut buf, "{:.1} {:.1} {:.1} {:.1} {:.1} {:.1} c S\n",
        cx - rx, cy - ry * k, cx - rx * k, cy - ry, cx, cy - ry).unwrap();

    buf
}

/// Polygon or polyline: stroked/filled closed path. [FR-ANNOT-1]
fn generate_polygon_appearance(
    points: &[crate::annotation::Point],
    color: &Color,
    border: &BorderStyle,
    closed: bool,
) -> Vec<u8> {
    let mut buf = Vec::new();
    let c = color;

    if points.is_empty() {
        return buf;
    }

    write!(&mut buf, "{:.3} {:.3} {:.3} rg\n", c.r, c.g, c.b).unwrap();
    write!(&mut buf, "{:.1} w\n", border.width).unwrap();

    write!(&mut buf, "{:.1} {:.1} m\n", points[0].x, points[0].y).unwrap();
    for pt in &points[1..] {
        write!(&mut buf, "{:.1} {:.1} l\n", pt.x, pt.y).unwrap();
    }

    if closed {
        write!(&mut buf, "h S\n").unwrap(); // close + stroke
    } else {
        write!(&mut buf, "S\n").unwrap(); // stroke
    }

    buf
}

/// Stamp: text label in a box. [FR-ANNOT-1]
fn generate_stamp_appearance(rect: &Rect, text: &str) -> Vec<u8> {
    let mut buf = Vec::new();

    // Red background for stamps.
    write!(&mut buf, "1 0 0 rg\n").unwrap();
    write!(&mut buf, "{:.1} {:.1} {:.1} {:.1} re f\n",
        rect.x, rect.y, rect.width, rect.height).unwrap();

    // White border.
    write!(&mut buf, "1 1 1 rg\n").unwrap();
    write!(&mut buf, "2 w\n").unwrap();
    write!(&mut buf, "{:.1} {:.1} {:.1} {:.1} re S\n",
        rect.x, rect.y, rect.width, rect.height).unwrap();

    // Text.
    if !text.is_empty() {
        write!(&mut buf, "BT\n").unwrap();
        write!(&mut buf, "/F1 14 Tf\n").unwrap();
        write!(&mut buf, "1 1 1 rg\n").unwrap(); // white text
        let escaped = escape_pdf_string(text);
        write!(&mut buf, "{:.1} {:.1} Td\n", rect.x + 4.0, rect.y + rect.height / 2.0).unwrap();
        write!(&mut buf, "({}) Tj\n", escaped).unwrap();
        write!(&mut buf, "ET\n").unwrap();
    }

    buf
}

/// Redaction: black filled rectangle. [FR-ANNOT-1]
fn generate_redaction_appearance(rect: &Rect, color: &Color) -> Vec<u8> {
    let mut buf = Vec::new();
    let c = color;

    write!(&mut buf, "{:.3} {:.3} {:.3} rg\n", c.r, c.g, c.b).unwrap();
    write!(&mut buf, "{:.1} {:.1} {:.1} {:.1} re f\n",
        rect.x, rect.y, rect.width, rect.height).unwrap();

    // Optional label.
    write!(&mut buf, "BT\n").unwrap();
    write!(&mut buf, "/F1 10 Tf\n").unwrap();
    write!(&mut buf, "1 1 1 rg\n").unwrap();
    write!(&mut buf, "{:.1} {:.1} Td\n", rect.x + 4.0, rect.y + rect.height / 2.0).unwrap();
    write!(&mut buf, "(Redacted) Tj\n").unwrap();
    write!(&mut buf, "ET\n").unwrap();

    buf
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Form widget appearances [FR-FORM-1, ADR-017, SDS §14 M5]
// ---------------------------------------------------------------------------

/// PDF object pair for a filled form widget (field dict + appearance stream).
#[derive(Debug, Clone)]
pub struct WidgetPdfObjects {
    /// 1-based object number for the widget / field dictionary.
    pub widget_obj_num: u32,
    /// 1-based object number for the appearance stream.
    pub ap_obj_num: u32,
    /// Serialized widget annotation / field dictionary bytes.
    pub widget_bytes: Vec<u8>,
    /// Serialized appearance stream object bytes.
    pub ap_bytes: Vec<u8>,
}

/// Generate a widget appearance stream for a form field. [FR-FORM-1]
///
/// Content is in **local** form-XObject coordinates with origin at the
/// bottom-left of the widget rect (BBox `[0 0 width height]`).
pub fn generate_widget_appearance(field: &FormField) -> Vec<u8> {
    match field.field_type {
        FieldType::Checkbox | FieldType::RadioButton => generate_checkbox_widget_appearance(field),
        FieldType::Button => generate_button_widget_appearance(field),
        FieldType::Signature => generate_signature_widget_appearance(field),
        FieldType::Text | FieldType::ComboBox | FieldType::ListBox => {
            generate_text_widget_appearance(field)
        }
    }
}

/// Ensure `field.appearance` is populated. Always regenerates from current value.
pub fn ensure_widget_appearance(field: &mut FormField) {
    field.appearance = Some(generate_widget_appearance(field));
}

/// Build PDF objects for a form widget with always-written `/AP`. [FR-FORM-1]
pub fn build_widget_pdf_objects(
    field: &mut FormField,
    widget_obj_num: u32,
    ap_obj_num: u32,
) -> WidgetPdfObjects {
    ensure_widget_appearance(field);
    let content = field
        .appearance
        .as_ref()
        .expect("ensure_widget_appearance always sets appearance")
        .clone();
    let rect = Rect::new(field.rect.x, field.rect.y, field.rect.width, field.rect.height);
    let ap_bytes = serialize_appearance_stream_object(ap_obj_num, &rect, &content);
    let widget_bytes = serialize_widget_dict_object(field, widget_obj_num, ap_obj_num);
    field.widget_obj_num = Some(widget_obj_num);
    WidgetPdfObjects {
        widget_obj_num,
        ap_obj_num,
        widget_bytes,
        ap_bytes,
    }
}

fn generate_text_widget_appearance(field: &FormField) -> Vec<u8> {
    let mut buf = Vec::new();
    let w = field.rect.width;
    let h = field.rect.height;
    let font_size = field.font_size.unwrap_or(10.0).clamp(6.0, 24.0);

    // Background + border in local coords.
    write!(&mut buf, "1 1 1 rg\n").unwrap();
    write!(&mut buf, "0 0 {w:.1} {h:.1} re f\n").unwrap();
    write!(&mut buf, "0 0 0 RG\n").unwrap();
    write!(&mut buf, "0.5 w\n").unwrap();
    write!(&mut buf, "0 0 {w:.1} {h:.1} re S\n").unwrap();

    let display = match &field.value {
        FieldValue::None => String::new(),
        other => {
            if field.password {
                "•".repeat(other.display().chars().count().min(32))
            } else {
                other.display()
            }
        }
    };

    if !display.is_empty() {
        write!(&mut buf, "BT\n").unwrap();
        write!(&mut buf, "/F1 {font_size:.1} Tf\n").unwrap();
        write!(&mut buf, "0 0 0 rg\n").unwrap();
        let escaped = escape_pdf_string(&display);
        let baseline = (h - font_size).max(2.0);
        write!(&mut buf, "2.0 {baseline:.1} Td\n").unwrap();
        write!(&mut buf, "({escaped}) Tj\n").unwrap();
        write!(&mut buf, "ET\n").unwrap();
    }
    buf
}

fn generate_checkbox_widget_appearance(field: &FormField) -> Vec<u8> {
    let mut buf = Vec::new();
    let w = field.rect.width.max(8.0);
    let h = field.rect.height.max(8.0);

    write!(&mut buf, "1 1 1 rg\n").unwrap();
    write!(&mut buf, "0 0 {w:.1} {h:.1} re f\n").unwrap();
    write!(&mut buf, "0 0 0 RG\n").unwrap();
    write!(&mut buf, "1 w\n").unwrap();
    write!(&mut buf, "0 0 {w:.1} {h:.1} re S\n").unwrap();

    let checked = matches!(field.value, FieldValue::Bool(true))
        || matches!(&field.value, FieldValue::Choice(s) if s != "Off" && !s.is_empty())
        || matches!(&field.value, FieldValue::Text(s) if s == "Yes" || s == "On" || s == "1");

    if checked {
        // Simple check mark (two strokes).
        write!(&mut buf, "0 0 0 RG\n").unwrap();
        write!(&mut buf, "1.5 w\n").unwrap();
        write!(&mut buf, "{:.1} {:.1} m\n", w * 0.2, h * 0.5).unwrap();
        write!(&mut buf, "{:.1} {:.1} l\n", w * 0.4, h * 0.25).unwrap();
        write!(&mut buf, "{:.1} {:.1} l S\n", w * 0.8, h * 0.8).unwrap();
    }
    buf
}

fn generate_button_widget_appearance(field: &FormField) -> Vec<u8> {
    let mut buf = Vec::new();
    let w = field.rect.width;
    let h = field.rect.height;
    write!(&mut buf, "0.9 0.9 0.9 rg\n").unwrap();
    write!(&mut buf, "0 0 {w:.1} {h:.1} re f\n").unwrap();
    write!(&mut buf, "0 0 0 RG\n").unwrap();
    write!(&mut buf, "1 w\n").unwrap();
    write!(&mut buf, "0 0 {w:.1} {h:.1} re S\n").unwrap();
    let label = if field.value.is_empty() {
        field.name.clone()
    } else {
        field.value.display()
    };
    if !label.is_empty() {
        write!(&mut buf, "BT\n/F1 9 Tf\n0 0 0 rg\n").unwrap();
        write!(&mut buf, "2.0 {:.1} Td\n", (h - 10.0).max(2.0)).unwrap();
        write!(&mut buf, "({}) Tj\nET\n", escape_pdf_string(&label)).unwrap();
    }
    buf
}

fn generate_signature_widget_appearance(field: &FormField) -> Vec<u8> {
    let mut buf = Vec::new();
    let w = field.rect.width;
    let h = field.rect.height;
    write!(&mut buf, "1 1 1 rg\n0 0 {w:.1} {h:.1} re f\n").unwrap();
    write!(&mut buf, "0.5 0.5 0.5 RG\n0.5 w\n0 0 {w:.1} {h:.1} re S\n").unwrap();
    let label = if field.value.is_empty() {
        "Sign".to_string()
    } else {
        field.value.display()
    };
    write!(&mut buf, "BT\n/F1 9 Tf\n0.3 0.3 0.3 rg\n").unwrap();
    write!(&mut buf, "4.0 {:.1} Td\n", (h - 12.0).max(2.0)).unwrap();
    write!(&mut buf, "({}) Tj\nET\n", escape_pdf_string(&label)).unwrap();
    buf
}

fn serialize_widget_dict_object(field: &FormField, widget_obj_num: u32, ap_obj_num: u32) -> Vec<u8> {
    let ft = field.pdf_type_str();
    let name = escape_pdf_string(&field.fully_qualified_name);
    let value = match &field.value {
        FieldValue::None => None,
        FieldValue::Bool(true) => Some("Yes".to_string()),
        FieldValue::Bool(false) => Some("Off".to_string()),
        other => Some(other.display()),
    };
    let r = &field.rect;

    let mut out = Vec::new();
    let _ = writeln!(out, "{widget_obj_num} 0 obj");
    let _ = write!(out, "<< /Type /Annot /Subtype /Widget /FT /{ft} ");
    let _ = write!(
        out,
        "/Rect [{} {} {} {}] ",
        r.x,
        r.y,
        r.x + r.width,
        r.y + r.height
    );
    let _ = write!(out, "/T ({name}) ");
    if let Some(ref v) = value {
        let escaped = escape_pdf_string(v);
        // Name values for buttons use /Yes style; text uses strings.
        if matches!(field.field_type, FieldType::Checkbox | FieldType::RadioButton) {
            let _ = write!(out, "/V /{} ", escaped.replace(' ', "_"));
            let _ = write!(out, "/AS /{} ", escaped.replace(' ', "_"));
        } else {
            let _ = write!(out, "/V ({escaped}) ");
        }
    }
    // Always-written appearance: never emit appearance-less widgets. [FR-FORM-1]
    let _ = write!(out, "/AP << /N {ap_obj_num} 0 R >> ");
    let _ = writeln!(out, "/F 4 >>");
    let _ = writeln!(out, "endobj");
    out
}

/// Escape special characters for a PDF literal string.
fn escape_pdf_string(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '(' => result.push_str("\\("),
            ')' => result.push_str("\\)"),
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            _ => result.push(ch),
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::annotation::{AnnotationType, Rect as AnnRect};

    #[test]
    fn text_markup_highlight_appearance() {
        let mut ann = Annotation::new(1, 0,
            AnnotationType::TextMarkup(TextMarkupKind::Highlight),
            AnnRect::new(10.0, 20.0, 100.0, 12.0));
        ann.properties.color = Color::new(1.0, 1.0, 0.0, 0.5);

        let appearance = generate_appearance(&ann);
        let text = String::from_utf8_lossy(&appearance);
        assert!(text.contains("re f")); // filled rectangle
    }

    #[test]
    fn text_markup_strikeout_line() {
        let mut ann = Annotation::new(1, 0,
            AnnotationType::TextMarkup(TextMarkupKind::Strikeout),
            AnnRect::new(10.0, 20.0, 100.0, 12.0));
        ann.properties.color = Color::new(1.0, 0.0, 0.0, 1.0);

        let appearance = generate_appearance(&ann);
        let text = String::from_utf8_lossy(&appearance);
        assert!(text.contains("l S")); // line stroke for strikeout
    }

    #[test]
    fn note_appearance() {
        let ann = Annotation::new(1, 0,
            AnnotationType::StickyNote,
            AnnRect::new(100.0, 200.0, 20.0, 20.0));

        let appearance = generate_appearance(&ann);
        assert!(!appearance.is_empty());
    }

    #[test]
    fn freetext_appearance_with_content() {
        let mut ann = Annotation::new(1, 0,
            AnnotationType::FreeText,
            AnnRect::new(10.0, 20.0, 200.0, 40.0));
        ann.properties.contents = "Hello World".into();

        let appearance = generate_appearance(&ann);
        let text = String::from_utf8_lossy(&appearance);
        assert!(text.contains("BT")); // text begin
        assert!(text.contains("Hello World"));
        assert!(text.contains("ET")); // text end
    }

    #[test]
    fn ink_appearance() {
        let mut ann = Annotation::new(1, 0,
            AnnotationType::Ink,
            AnnRect::new(0.0, 0.0, 100.0, 100.0));
        ann.ink_points = vec![vec![
            crate::annotation::Point::new(10.0, 10.0),
            crate::annotation::Point::new(50.0, 50.0),
            crate::annotation::Point::new(90.0, 20.0),
        ]];

        let appearance = generate_appearance(&ann);
        let text = String::from_utf8_lossy(&appearance);
        assert!(text.contains("m")); // moveto
        assert!(text.contains("l")); // lineto
        assert!(text.contains("S")); // stroke
    }

    #[test]
    fn line_appearance() {
        let mut ann = Annotation::new(1, 0,
            AnnotationType::Line,
            AnnRect::new(10.0, 10.0, 90.0, 90.0));
        ann.shape_points = vec![
            crate::annotation::Point::new(10.0, 10.0),
            crate::annotation::Point::new(90.0, 90.0),
        ];

        let appearance = generate_appearance(&ann);
        let text = String::from_utf8_lossy(&appearance);
        assert!(text.contains("m"));
        assert!(text.contains("l S"));
    }

    #[test]
    fn arrow_appearance() {
        let mut ann = Annotation::new(1, 0,
            AnnotationType::Arrow,
            AnnRect::new(10.0, 10.0, 90.0, 90.0));
        ann.shape_points = vec![
            crate::annotation::Point::new(10.0, 10.0),
            crate::annotation::Point::new(90.0, 90.0),
        ];

        let appearance = generate_appearance(&ann);
        let text = String::from_utf8_lossy(&appearance);
        // Arrowhead is a filled triangle.
        assert!(text.contains("f"));
    }

    #[test]
    fn rectangle_appearance() {
        let ann = Annotation::new(1, 0,
            AnnotationType::Rectangle,
            AnnRect::new(10.0, 20.0, 100.0, 50.0));

        let appearance = generate_appearance(&ann);
        let text = String::from_utf8_lossy(&appearance);
        assert!(text.contains("re S")); // rectangle stroke
    }

    #[test]
    fn ellipse_appearance() {
        let ann = Annotation::new(1, 0,
            AnnotationType::Ellipse,
            AnnRect::new(10.0, 20.0, 100.0, 50.0));

        let appearance = generate_appearance(&ann);
        let text = String::from_utf8_lossy(&appearance);
        assert!(text.contains("c")); // cubic bezier curves
    }

    #[test]
    fn stamp_appearance() {
        let mut ann = Annotation::new(1, 0,
            AnnotationType::Stamp,
            AnnRect::new(10.0, 20.0, 100.0, 30.0));
        ann.properties.contents = "APPROVED".into();

        let appearance = generate_appearance(&ann);
        let text = String::from_utf8_lossy(&appearance);
        assert!(text.contains("APPROVED"));
        assert!(text.contains("1 0 0 rg")); // red
    }

    #[test]
    fn redaction_appearance() {
        let ann = Annotation::new(1, 0,
            AnnotationType::Redaction,
            AnnRect::new(10.0, 20.0, 100.0, 30.0));

        let appearance = generate_appearance(&ann);
        let text = String::from_utf8_lossy(&appearance);
        assert!(text.contains("Redacted"));
    }

    #[test]
        fn escape_pdf_string_specials() {
        assert_eq!(escape_pdf_string("hello"), "hello");
        assert_eq!(escape_pdf_string("a(b)c"), "a\\(b\\)c");
        assert_eq!(escape_pdf_string("back\\slash"), "back\\\\slash");
        assert_eq!(escape_pdf_string("new\nline"), "new\\nline");
    }

    #[test]
    fn widget_text_appearance_contains_value() {
        // [FR-FORM-1] filled text fields get an always-written appearance.
        use crate::form::{FieldRect, FieldType, FieldValue, FormField};
        let mut field =
            FormField::new("amount", FieldType::Text, 0, FieldRect::new(10.0, 20.0, 100.0, 18.0));
        field.set_value(FieldValue::Text("42.50".into()));
        let ap = generate_widget_appearance(&field);
        let text = String::from_utf8_lossy(&ap);
        assert!(text.contains("42.50"), "missing value in AP: {text}");
        assert!(text.contains("BT"), "expected text operators");
    }

    #[test]
    fn widget_checkbox_checked_draws_mark() {
        use crate::form::{FieldRect, FieldType, FieldValue, FormField};
        let mut field =
            FormField::new("agree", FieldType::Checkbox, 0, FieldRect::new(0.0, 0.0, 14.0, 14.0));
        field.set_value(FieldValue::Bool(true));
        let ap = generate_widget_appearance(&field);
        let text = String::from_utf8_lossy(&ap);
        assert!(
            text.contains(" l ") || text.contains(" l\n") || text.contains("l S"),
            "expected check path: {text}"
        );
    }

    #[test]
    fn build_widget_pdf_objects_always_writes_ap() {
        // [FR-FORM-1] widget dict always references /AP.
        use crate::form::{FieldRect, FieldType, FieldValue, FormField};
        let mut field =
            FormField::new("name", FieldType::Text, 0, FieldRect::new(50.0, 700.0, 120.0, 16.0));
        field.set_value(FieldValue::Text("Ada".into()));
        assert!(field.appearance.is_none());
        let objs = build_widget_pdf_objects(&mut field, 40, 41);
        assert!(field.appearance.is_some());
        assert_eq!(objs.widget_obj_num, 40);
        assert_eq!(objs.ap_obj_num, 41);
        let widget_text = String::from_utf8_lossy(&objs.widget_bytes);
        assert!(
            widget_text.contains("/AP << /N 41 0 R >>"),
            "widget missing /AP: {widget_text}"
        );
        assert!(widget_text.contains("/T (name)"));
        assert!(widget_text.contains("/V (Ada)"));
        let ap_text = String::from_utf8_lossy(&objs.ap_bytes);
        assert!(ap_text.contains("41 0 obj"));
        assert!(ap_text.contains("/Subtype /Form"));
        assert!(ap_text.contains("Ada"));
    }

    #[test]
    fn build_annotation_pdf_objects_always_writes_ap() {
        // [FR-ANNOT-2] every written annotation must embed /AP
        let mut ann = Annotation::new(
            1,
            0,
            AnnotationType::StickyNote,
            AnnRect::new(10.0, 20.0, 24.0, 24.0),
        )
        .with_contents("hello");
        assert!(!ann.has_appearance());

        let objs = build_annotation_pdf_objects(&mut ann, 50, 51);
        assert!(ann.has_appearance());
        assert_eq!(objs.annot_obj_num, 50);
        assert_eq!(objs.ap_obj_num, 51);

        let annot_text = String::from_utf8_lossy(&objs.annot_bytes);
        assert!(
            annot_text.contains("/AP << /N 51 0 R >>"),
            "annot missing /AP: {annot_text}"
        );
        assert!(annot_text.contains("/Subtype /Text"));
        assert!(annot_text.contains("50 0 obj"));

        let ap_text = String::from_utf8_lossy(&objs.ap_bytes);
        assert!(ap_text.contains("51 0 obj"));
        assert!(ap_text.contains("/Subtype /Form"));
        assert!(ap_text.contains("stream"));
        assert!(ap_text.contains("endstream"));
    }
}
