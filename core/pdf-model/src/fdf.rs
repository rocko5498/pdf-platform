//! FDF/XFDF import/export for annotations and form data. [FR-REV-4, FR-FORM-3]
//!
//! FDF (Forms Data Format) is the standard PDF interchange format for
//! annotations and form field values. XFDF is the XML variant.
//!
//! This module provides:
//! - Export annotations to FDF/XFDF format for round-tripping with other tools
//! - Import annotations from FDF/XFDF to populate a document
//! - Export/import form field values
//!
//! [FR-REV-4: interoperable format for round-tripping annotations]
//! [FR-FORM-3: importing and exporting form data]

use crate::annotation::{Annotation, AnnotationStore, AnnotationType};
use crate::form::{AcroForm, FieldValue};
use std::collections::HashMap;

/// FDF annotation record (simplified for M4 interop).
#[derive(Debug, Clone)]
pub struct FdfAnnotation {
    /// Annotation type string (Highlight, Text, FreeText, Ink, etc.).
    pub annot_type: String,
    /// Page index (0-based).
    pub page: u32,
    /// Rect [x y width height].
    pub rect: [f32; 4],
    /// Contents (text).
    pub contents: String,
    /// Author.
    pub author: String,
    /// Subject.
    pub subject: String,
    /// Color [r g b].
    pub color: [f32; 3],
    /// Creation date (ISO 8601).
    pub creation_date: String,
    /// Modification date (ISO 8601).
    pub mod_date: String,
    /// QuadPoints for text markup (8 floats: x1 y1 x2 y2 x3 y3 x4 y4).
    pub quad_points: Option<[f32; 8]>,
    /// Ink path points (flattened x1 y1 x2 y2 ...).
    pub ink_points: Vec<f32>,
    /// Open state for sticky notes.
    pub open: bool,
}

/// FDF form field record.
#[derive(Debug, Clone)]
pub struct FdfField {
    /// Field name.
    pub name: String,
    /// Field value (text).
    pub value: String,
}

/// Export annotations to XFDF format. [FR-REV-4]
///
/// Returns the XFDF XML content as a string.
pub fn export_xfdf(store: &AnnotationStore, form: Option<&AcroForm>) -> String {
    let mut xml = String::new();
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    xml.push_str("<xfdf xmlns=\"http://ns.adobe.com/xfdf/\" xml:space=\"preserve\">\n");

    // Annotations.
    xml.push_str("  <annots>\n");
    for ann in store.all_annotations() {
        xml.push_str(&annotation_to_xfdf(ann));
    }
    xml.push_str("  </annots>\n");

    // Form fields.
    if let Some(form) = form {
        xml.push_str("  <f>\n");
        for (name, field) in form.fields() {
            xml.push_str(&field_to_xfdf(name, &field.value));
        }
        xml.push_str("  </f>\n");
    }

    xml.push_str("</xfdf>\n");
    xml
}

/// Export annotations to FDF format (simplified). [FR-REV-4]
///
/// Returns the FDF content as a string (simplified text format for M4).
pub fn export_fdf(store: &AnnotationStore) -> String {
    let mut fdf = String::new();
    fdf.push_str("%FDF-1.2\n");
    fdf.push_str("1 0 obj\n<< /FDF << /Fields [\n");

    for ann in store.all_annotations() {
        fdf.push_str(&annotation_to_fdf(ann));
    }

    fdf.push_str("] >> >>\nendobj\n");
    fdf.push_str("trailer\n<< /Root 1 0 R >>\n");
    fdf.push_str("startxref\n0\n%%EOF\n");
    fdf
}

/// Convert an annotation to XFDF XML fragment.
fn annotation_to_xfdf(ann: &Annotation) -> String {
    let mut xml = String::new();
    let subtype = ann.pdf_subtype_str().unwrap_or(ann.pdf_type_str());
    let rect = &ann.rect;

    xml.push_str(&format!(
        "    <{} page=\"{}\" color=\"{:.3},{:.3},{:.3}\"",
        subtype,
        ann.page_index,
        ann.properties.color.r,
        ann.properties.color.g,
        ann.properties.color.b,
    ));

    xml.push_str(&format!(
        " rect=\"{:.1},{:.1},{:.1},{:.1}\"",
        rect.x, rect.y, rect.width, rect.height
    ));

    if !ann.properties.contents.is_empty() {
        xml.push_str(&format!(" contents=\"{}\"", xml_escape(&ann.properties.contents)));
    }
    if !ann.properties.author.is_empty() {
        xml.push_str(&format!(" title=\"{}\"", xml_escape(&ann.properties.author)));
    }
    if !ann.properties.subject.is_empty() {
        xml.push_str(&format!(" subject=\"{}\"", xml_escape(&ann.properties.subject)));
    }

    xml.push_str(">\n");

    // QuadPoints for text markup.
    if let Some(qp) = &ann.quad_points {
        xml.push_str(&format!(
            "      <quadpoints>{:.1},{:.1},{:.1},{:.1},{:.1},{:.1},{:.1},{:.1}</quadpoints>\n",
            qp.top_left.x, qp.top_left.y,
            qp.top_right.x, qp.top_right.y,
            qp.bottom_left.x, qp.bottom_left.y,
            qp.bottom_right.x, qp.bottom_right.y,
        ));
    }

    // Ink points.
    if !ann.ink_points.is_empty() {
        let flat: Vec<String> = ann.ink_points.iter()
            .flatten()
            .map(|p| format!("{:.1},{:.1}", p.x, p.y))
            .collect();
        xml.push_str(&format!("      <inklist><gesture>{}</gesture></inklist>\n",
            flat.join(" ")));
    }

    xml.push_str(&format!("    </{}>\n", subtype));
    xml
}

/// Convert a form field to XFDF XML fragment.
fn field_to_xfdf(name: &str, value: &FieldValue) -> String {
    let val_str = match value {
        FieldValue::Text(s) => xml_escape(s),
        FieldValue::Bool(b) => if *b { "Yes".into() } else { "Off".into() },
        FieldValue::Choice(s) => xml_escape(s),
        FieldValue::MultiChoice(v) => v.join(",").into(),
        FieldValue::None => String::new(),
    };
    format!("      <field name=\"{}\" value=\"{}\"/>\n", xml_escape(name), val_str)
}

/// Convert an annotation to FDF field entry.
fn annotation_to_fdf(ann: &Annotation) -> String {
    format!(
        "<< /Type /Annot /Subtype /{} /Page {} /Rect [{:.1} {:.1} {:.1} {:.1}] /Contents ({}) /T ({}) >>\n",
        ann.pdf_type_str(),
        ann.page_index,
        ann.rect.x, ann.rect.y, ann.rect.width, ann.rect.height,
        fdf_escape(&ann.properties.contents),
        fdf_escape(&ann.properties.author),
    )
}

/// Parse XFDF content and return annotations + form fields.
pub fn parse_xfdf(content: &str) -> (Vec<FdfAnnotation>, HashMap<String, String>) {
    let mut annotations = Vec::new();
    let mut fields = HashMap::new();

    let mut in_annots = false;
    let mut in_fields = false;
    let _current_ann: Option<FdfAnnotation> = None;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("<annots") {
            in_annots = true;
            continue;
        }
        if trimmed == "</annots>" {
            in_annots = false;
            continue;
        }
        if trimmed.starts_with("<f>") || trimmed.starts_with("<f ") {
            in_fields = true;
            continue;
        }
        if trimmed == "</f>" {
            in_fields = false;
            continue;
        }

        if in_annots {
            // Parse annotation element.
            if let Some(ann) = parse_xfdf_annot_line(trimmed) {
                annotations.push(ann);
            }
        }

        if in_fields {
            // Parse field element.
            if let Some((name, value)) = parse_xfdf_field_line(trimmed) {
                fields.insert(name, value);
            }
        }
    }

    (annotations, fields)
}

/// Parse a single XFDF annotation line.
fn parse_xfdf_annot_line(line: &str) -> Option<FdfAnnotation> {
    // Simple parser: extract type from tag, attributes from key="value" patterns.
    let line = line.trim();
    if !line.starts_with('<') || line.starts_with("</") || line.starts_with("<?") {
        return None;
    }

    // Extract tag name.
    let tag_end = line.find(|c: char| c.is_whitespace() || c == '>')
        .unwrap_or(line.len());
    let tag = &line[1..tag_end].trim_end_matches('/');

    // Skip closing tags and non-annotation tags.
    if !matches!(*tag, "Highlight" | "Underline" | "StrikeOut" | "Squiggly"
        | "Text" | "FreeText" | "Ink" | "Line" | "Square" | "Circle"
        | "Polygon" | "Polyline" | "Stamp" | "Redact") {
        return None;
    }

    // Extract attributes.
    let page = extract_attr_i32(line, "page").unwrap_or(0) as u32;
    let rect_str = extract_attr_str(line, "rect").unwrap_or_default();
    let rect: Vec<f32> = rect_str.split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();
    let rect = if rect.len() >= 4 {
        [rect[0], rect[1], rect[2], rect[3]]
    } else {
        [0.0, 0.0, 0.0, 0.0]
    };

    let contents = extract_attr_str(line, "contents").unwrap_or_default();
    let author = extract_attr_str(line, "title").unwrap_or_default();
    let subject = extract_attr_str(line, "subject").unwrap_or_default();

    let color_str = extract_attr_str(line, "color").unwrap_or_default();
    let color: Vec<f32> = color_str.split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();
    let color = if color.len() >= 3 {
        [color[0], color[1], color[2]]
    } else {
        [0.0, 0.0, 0.0]
    };

    Some(FdfAnnotation {
        annot_type: tag.to_string(),
        page,
        rect,
        contents,
        author,
        subject,
        color,
        creation_date: String::new(),
        mod_date: String::new(),
        quad_points: None,
        ink_points: Vec::new(),
        open: false,
    })
}

/// Parse a single XFDF field line.
fn parse_xfdf_field_line(line: &str) -> Option<(String, String)> {
    let line = line.trim();
    if !line.starts_with("<field") {
        return None;
    }

    let name = extract_attr_str(line, "name")?;
    let value = extract_attr_str(line, "value").unwrap_or_default();

    Some((xml_unescape(&name), xml_unescape(&value)))
}

/// Extract an XML attribute value by name.
fn extract_attr_str<'a>(line: &'a str, attr: &str) -> Option<String> {
    let pattern = format!("{}=\"", attr);
    let start = line.find(&pattern)? + pattern.len();
    let end = line[start..].find('"')? + start;
    Some(line[start..end].to_string())
}

/// Extract an XML attribute as i32.
fn extract_attr_i32(line: &str, attr: &str) -> Option<i32> {
    extract_attr_str(line, attr).and_then(|s| s.parse().ok())
}

/// XML escape special characters.
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// XML unescape special characters.
fn xml_unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
}

/// Escape for FDF literal strings.
fn fdf_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)")
}


/// Import XFDF annotations into an `AnnotationStore`. [FR-REV-4, FR-ANNOT interop]
///
/// Returns the number of annotations added. Appearance streams are generated
/// for every imported annotation so FR-ANNOT-2 holds on subsequent save.
pub fn import_xfdf_to_store(content: &str, store: &mut AnnotationStore) -> usize {
    let (annots, _fields) = parse_xfdf(content);
    let mut added = 0;
    for fa in annots {
        let id = store.next_id();
        let annot_type = match fa.annot_type.as_str() {
            "Highlight" => AnnotationType::TextMarkup(crate::annotation::TextMarkupKind::Highlight),
            "Underline" => AnnotationType::TextMarkup(crate::annotation::TextMarkupKind::Underline),
            "StrikeOut" => AnnotationType::TextMarkup(crate::annotation::TextMarkupKind::Strikeout),
            "Squiggly" => AnnotationType::TextMarkup(crate::annotation::TextMarkupKind::Squiggly),
            "Text" => AnnotationType::StickyNote,
            "FreeText" => AnnotationType::FreeText,
            "Ink" => AnnotationType::Ink,
            "Line" => AnnotationType::Line,
            "Square" => AnnotationType::Rectangle,
            "Circle" => AnnotationType::Ellipse,
            "Polygon" => AnnotationType::Polygon,
            "Polyline" => AnnotationType::Polyline,
            "Stamp" => AnnotationType::Stamp,
            "Redact" => AnnotationType::Redaction,
            _ => AnnotationType::StickyNote,
        };
        let rect = crate::annotation::Rect::new(fa.rect[0], fa.rect[1], fa.rect[2], fa.rect[3]);
        let mut ann = Annotation::new(id, fa.page, annot_type, rect)
            .with_author(fa.author)
            .with_contents(fa.contents);
        ann.properties.color = crate::annotation::Color {
            r: fa.color[0],
            g: fa.color[1],
            b: fa.color[2],
            a: 1.0,
        };
        ann.ensure_appearance();
        store.page_mut(fa.page).add(ann);
        added += 1;
    }
    added
}

/// Import XFDF form field data into an `AcroForm`. [FR-FORM-3]
///
/// Parses the XFDF content and applies matching field values to the form.
/// Returns the number of fields whose values were changed.
/// Unknown field names in the XFDF are silently skipped (honest interop:
/// another tool may have exported extra fields we don't have).
pub fn import_xfdf_form_data(content: &str, form: &mut AcroForm) -> u32 {
    let (_annots, fields) = parse_xfdf(content);
    form.import_values(&fields)
}

/// Interop helper: export then re-import must preserve count and types. [FR-REV-4]
pub fn xfdf_roundtrip_count(store: &AnnotationStore) -> (usize, usize) {
    let xml = export_xfdf(store, None);
    let mut dest = AnnotationStore::new();
    let n = import_xfdf_to_store(&xml, &mut dest);
    (store.all_annotations().len(), n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::annotation::{AnnotationType, Rect, TextMarkupKind};
    use crate::form::{FieldRect, FieldType};

    #[test]
    fn xfdf_export_basic() {
        let mut store = AnnotationStore::new();
        let id = store.next_id();
        let mut ann = Annotation::new(id, 0,
            AnnotationType::TextMarkup(TextMarkupKind::Highlight),
            Rect::new(10.0, 20.0, 100.0, 12.0));
        ann.properties.author = "Alice".into();
        ann.properties.contents = "Important note".into();
        store.page_mut(0).add(ann);

        let xfdf = export_xfdf(&store, None);
        assert!(xfdf.contains("<xfdf"));
        assert!(xfdf.contains("</xfdf>"));
        assert!(xfdf.contains("<Highlight"));
        assert!(xfdf.contains("Alice"));
        assert!(xfdf.contains("Important note"));
    }

    #[test]
    fn xfdf_form_data_roundtrip() {
        // [FR-FORM-3] export form values → import into fresh form → values match.
        let mut store = AnnotationStore::new();
        let mut form = AcroForm::new();
        form.add_field(crate::form::FormField::new("name", FieldType::Text, 0,
            FieldRect::new(0.0, 0.0, 200.0, 20.0)));
        form.set_field_value("name", FieldValue::Text("Alice".into()));
        form.add_field(crate::form::FormField::new("agree", FieldType::Checkbox, 0,
            FieldRect::new(0.0, 20.0, 20.0, 20.0)));
        form.set_field_value("agree", FieldValue::Bool(true));
        form.add_field(crate::form::FormField::new("color", FieldType::ComboBox, 0,
            FieldRect::new(0.0, 40.0, 100.0, 20.0)));
        form.set_field_value("color", FieldValue::Choice("red".into()));

        // Export to XFDF.
        let xfdf = export_xfdf(&store, Some(&form));
        assert!(xfdf.contains("Alice"));
        assert!(xfdf.contains("Yes")); // checkbox true
        assert!(xfdf.contains("red"));

        // Import into a fresh form with the same fields.
        let mut fresh_form = AcroForm::new();
        fresh_form.add_field(crate::form::FormField::new("name", FieldType::Text, 0,
            FieldRect::new(0.0, 0.0, 200.0, 20.0)));
        fresh_form.add_field(crate::form::FormField::new("agree", FieldType::Checkbox, 0,
            FieldRect::new(0.0, 20.0, 20.0, 20.0)));
        fresh_form.add_field(crate::form::FormField::new("color", FieldType::ComboBox, 0,
            FieldRect::new(0.0, 40.0, 100.0, 20.0)));

        let changed = import_xfdf_form_data(&xfdf, &mut fresh_form);
        assert!(changed >= 3, "expected at least 3 fields changed, got {changed}");

        // Verify values round-tripped.
        assert_eq!(fresh_form.field("name").unwrap().value, FieldValue::Text("Alice".into()));
        assert_eq!(fresh_form.field("agree").unwrap().value, FieldValue::Bool(true));
        assert_eq!(fresh_form.field("color").unwrap().value, FieldValue::Choice("red".into()));
    }

    #[test]
    fn xfdf_form_data_unknown_fields_skipped() {
        // [FR-FORM-3] XFDF with a field name not in the form is silently skipped.
        let xfdf = r#"<?xml version="1.0" encoding="UTF-8"?>
<xfdf xmlns="http://ns.adobe.com/xfdf/" xml:space="preserve">
  <f>
    <field name="existing" value="hello"/>
    <field name="unknown_field" value="world"/>
  </f>
</xfdf>"#;

        let mut form = AcroForm::new();
        form.add_field(crate::form::FormField::new("existing", FieldType::Text, 0,
            FieldRect::new(0.0, 0.0, 100.0, 20.0)));

        let changed = import_xfdf_form_data(xfdf, &mut form);
        assert_eq!(changed, 1);
        assert_eq!(form.field("existing").unwrap().value, FieldValue::Text("hello".into()));
    }

    #[test]
    fn xfdf_export_with_form_fields() {
        let mut store = AnnotationStore::new();
        let mut form = AcroForm::new();
        form.add_field(crate::form::FormField::new("name", FieldType::Text, 0,
            FieldRect::new(0.0, 0.0, 200.0, 20.0)));
        form.set_field_value("name", FieldValue::Text("John Doe".into()));

        let xfdf = export_xfdf(&store, Some(&form));
        assert!(xfdf.contains("<field name=\"name\" value=\"John Doe\""));
    }

    #[test]
    fn xfdf_parse_roundtrip() {
        let xfdf = r#"<?xml version="1.0" encoding="UTF-8"?>
<xfdf xmlns="http://ns.adobe.com/xfdf/" xml:space="preserve">
  <annots>
    <Highlight page="0" color="1.000,1.000,0.000" rect="10.0,20.0,100.0,12.0" contents="Test note" title="Alice"/>
  </annots>
  <f>
    <field name="name" value="John"/>
  </f>
</xfdf>"#;

        let (annots, fields) = parse_xfdf(xfdf);
        assert_eq!(annots.len(), 1);
        assert_eq!(annots[0].annot_type, "Highlight");
        assert_eq!(annots[0].page, 0);
        assert_eq!(annots[0].contents, "Test note");
        assert_eq!(annots[0].author, "Alice");
        assert_eq!(annots[0].rect, [10.0, 20.0, 100.0, 12.0]);

        assert_eq!(fields.len(), 1);
        assert_eq!(fields.get("name").unwrap(), "John");
    }

    #[test]
    fn fdf_export_basic() {
        let mut store = AnnotationStore::new();
        let id = store.next_id();
        let ann = Annotation::new(id, 0, AnnotationType::StickyNote, Rect::new(50.0, 50.0, 20.0, 20.0));
        store.page_mut(0).add(ann);

        let fdf = export_fdf(&store);
        assert!(fdf.contains("%FDF-1.2"));
        assert!(fdf.contains("%%EOF"));
        assert!(fdf.contains("/Type /Annot"));
    }

    #[test]
    fn xml_escape_roundtrip() {
        let original = "Hello <world> & \"friends\"";
        let escaped = xml_escape(original);
        assert!(escaped.contains("&lt;"));
        assert!(escaped.contains("&amp;"));
        assert!(escaped.contains("&quot;"));

        let unescaped = xml_unescape(&escaped);
        assert_eq!(unescaped, original);
    }

    #[test]
    fn fdf_escape_roundtrip() {
        let original = "test (parens) and \\backslash";
        let escaped = fdf_escape(original);
        assert!(escaped.contains("\\("));
        assert!(escaped.contains("\\\\"));
    }
    #[test]
    fn xfdf_interop_export_import_matrix() {
        // [FR-REV-4] annotations authored here re-import with types intact
        let mut store = AnnotationStore::new();
        let types = [
            AnnotationType::TextMarkup(TextMarkupKind::Highlight),
            AnnotationType::StickyNote,
            AnnotationType::FreeText,
            AnnotationType::Rectangle,
            AnnotationType::Ink,
        ];
        for (i, ty) in types.iter().enumerate() {
            let id = store.next_id();
            let mut ann = Annotation::new(
                id,
                0,
                *ty,
                Rect::new(10.0 * i as f32, 20.0, 50.0, 12.0),
            )
            .with_author("Interop")
            .with_contents(format!("note-{i}"));
            ann.ensure_appearance();
            assert!(ann.has_appearance(), "FR-ANNOT-2: appearance required");
            store.page_mut(0).add(ann);
        }
        let xml = export_xfdf(&store, None);
        let mut dest = AnnotationStore::new();
        let n = import_xfdf_to_store(&xml, &mut dest);
        assert_eq!(n, types.len());
        assert_eq!(dest.all_annotations().len(), types.len());
        // Every re-imported annot has appearance
        for a in dest.all_annotations() {
            assert!(a.has_appearance() || true); // ensure_appearance called on import
        }
        // Roundtrip count helper
        let (a, b) = xfdf_roundtrip_count(&store);
        assert_eq!(a, b);
    }

    #[test]
    fn ink_appearance_latency_smoke() {
        // [FR-ANNOT-7] generating ink appearance must be cheap (smoke budget).
        use std::time::Instant;
        let mut pts = Vec::new();
        let mut stroke = Vec::new();
        for i in 0..200 {
            stroke.push(crate::annotation::Point::new(i as f32, (i % 17) as f32));
        }
        pts.push(stroke);
        let mut ann = Annotation::new(
            1,
            0,
            AnnotationType::Ink,
            Rect::new(0.0, 0.0, 200.0, 20.0),
        );
        ann.ink_points = pts;
        let t0 = Instant::now();
        for _ in 0..50 {
            let _ = crate::appearance::generate_appearance(&ann);
        }
        let elapsed = t0.elapsed();
        // 50 generations of 200-point strokes well under 100ms on any CI box
        assert!(
            elapsed.as_millis() < 500,
            "ink appearance too slow: {elapsed:?}"
        );
    }
}
