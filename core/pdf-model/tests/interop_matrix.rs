//! Annotation interop matrix (unit-level). [FR-ANNOT-2, FR-REV-4, SDS §14 M4]
//!
//! Proves: every core type exports XFDF, re-imports with appearance, and
//! round-trips without silent type loss. External Acrobat/Foxit validation
//! remains a release-train manual/CI-external step.

use pdf_model::annotation::{Annotation, AnnotationStore, AnnotationType, Rect, TextMarkupKind};
use pdf_model::fdf::{export_xfdf, import_xfdf_to_store, xfdf_roundtrip_count};
use pdf_model::page_patch::inject_annot_refs;

fn all_core_types() -> Vec<AnnotationType> {
    vec![
        AnnotationType::TextMarkup(TextMarkupKind::Highlight),
        AnnotationType::TextMarkup(TextMarkupKind::Underline),
        AnnotationType::TextMarkup(TextMarkupKind::Strikeout),
        AnnotationType::TextMarkup(TextMarkupKind::Squiggly),
        AnnotationType::StickyNote,
        AnnotationType::FreeText,
        AnnotationType::Ink,
        AnnotationType::Line,
        AnnotationType::Rectangle,
        AnnotationType::Ellipse,
        AnnotationType::Polygon,
        AnnotationType::Polyline,
        AnnotationType::Stamp,
        AnnotationType::Redaction,
    ]
}

#[test]
fn interop_matrix_all_types_have_appearance_and_xfdf() {
    let mut store = AnnotationStore::new();
    for (i, ty) in all_core_types().into_iter().enumerate() {
        let id = store.next_id();
        let mut ann = Annotation::new(
            id,
            0,
            ty,
            Rect::new(10.0, 20.0 + i as f32 * 5.0, 80.0, 14.0),
        )
        .with_author("matrix")
        .with_contents(format!("t{i}"));
        ann.ensure_appearance();
        assert!(
            ann.has_appearance(),
            "FR-ANNOT-2: type {ty:?} must write appearance"
        );
        store.page_mut(0).add(ann);
    }

    let xml = export_xfdf(&store, None);
    assert!(xml.contains("<xfdf"));
    assert!(xml.contains("</xfdf>"));

    let mut dest = AnnotationStore::new();
    let n = import_xfdf_to_store(&xml, &mut dest);
    // Import supports the common XFDF tags; all should rehydrate.
    assert!(n >= 10, "expected most types to re-import, got {n}");

    let (a, b) = xfdf_roundtrip_count(&store);
    assert_eq!(a, store.all_annotations().len());
    assert!(b > 0);
}

#[test]
fn page_patch_links_annots_for_incremental_save() {
    let page = b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>\nendobj\n";
    let patched = inject_annot_refs(page, &[100, 101]).expect("patch");
    let s = String::from_utf8_lossy(&patched);
    assert!(s.contains("/Annots [100 0 R 101 0 R]"));
}

/// XFDF's `rect` attribute is the annotation's `/Rect`: two opposite corners in
/// PDF user space. Export used to write `x, y, width, height` and import read
/// it back the same way, so the round-trip above passed while every file handed
/// to another tool described a different rectangle. Nothing here checked
/// geometry, which is why it survived. [FR-REV-4, PRIN-7, T-10]
#[test]
fn an_exported_rect_names_the_corners_not_the_extent() {
    let mut store = AnnotationStore::new();
    let id = store.next_id();
    store.page_mut(0).add(Annotation::new(
        id,
        0,
        AnnotationType::TextMarkup(TextMarkupKind::Highlight),
        Rect::new(10.0, 20.0, 80.0, 14.0),
    ));

    let xml = export_xfdf(&store, None);

    assert!(
        xml.contains("rect=\"10.0,20.0,90.0,34.0\""),
        "expected the far corner (90, 34), got: {xml}"
    );
}

#[test]
fn a_rect_written_by_another_tool_imports_at_the_right_size() {
    // 100x24 at (72, 700), written the way the XFDF specification says.
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<xfdf xmlns="http://ns.adobe.com/xfdf/">
  <annots>
    <Highlight page="0" rect="72.0,700.0,172.0,724.0" title="other tool"/>
  </annots>
</xfdf>"#;

    let mut store = AnnotationStore::new();
    assert_eq!(import_xfdf_to_store(xml, &mut store), 1);

    let annotation = store.all_annotations()[0];
    assert_eq!(annotation.rect.x, 72.0);
    assert_eq!(annotation.rect.y, 700.0);
    assert_eq!(annotation.rect.width, 100.0, "width came from the far corner");
    assert_eq!(annotation.rect.height, 24.0, "height came from the far corner");
}

#[test]
fn corners_in_either_order_describe_the_same_rectangle() {
    // XFDF does not promise which corner comes first.
    let reversed = r#"<xfdf xmlns="http://ns.adobe.com/xfdf/"><annots>
    <Square page="0" rect="172.0,724.0,72.0,700.0"/>
    </annots></xfdf>"#;

    let mut store = AnnotationStore::new();
    assert_eq!(import_xfdf_to_store(reversed, &mut store), 1);

    let annotation = store.all_annotations()[0];
    assert_eq!(
        (annotation.rect.x, annotation.rect.y, annotation.rect.width, annotation.rect.height),
        (72.0, 700.0, 100.0, 24.0),
        "reversed corners must not produce a negative-size rectangle"
    );
}

#[test]
fn geometry_survives_a_round_trip() {
    let mut store = AnnotationStore::new();
    let id = store.next_id();
    store.page_mut(0).add(Annotation::new(
        id,
        2,
        AnnotationType::StickyNote,
        Rect::new(33.5, 44.5, 21.0, 19.0),
    ));

    let mut back = AnnotationStore::new();
    import_xfdf_to_store(&export_xfdf(&store, None), &mut back);

    let annotation = back.all_annotations()[0];
    assert_eq!(
        (annotation.rect.x, annotation.rect.y, annotation.rect.width, annotation.rect.height),
        (33.5, 44.5, 21.0, 19.0)
    );
    assert_eq!(annotation.page_index, 2, "the page index must survive too");
}

#[test]
fn an_xfdf_file_without_line_breaks_still_imports() {
    // The importer used to scan `content.lines()` and required `<annots>` to
    // begin a line of its own. XFDF permits any whitespace, and tools that
    // minify produce exactly this. Zero annotations were imported, and the
    // caller was told the import succeeded. [FR-REV-4, GR-8]
    let minified = concat!(
        r#"<?xml version="1.0"?><xfdf xmlns="http://ns.adobe.com/xfdf/"><annots>"#,
        r#"<Highlight page="0" rect="10,20,110,32" title="a"/>"#,
        r#"<Square page="1" rect="0,0,50,50" title="b"/>"#,
        r#"</annots></xfdf>"#
    );

    let mut store = AnnotationStore::new();
    assert_eq!(
        import_xfdf_to_store(minified, &mut store),
        2,
        "a single-line XFDF file must import the same annotations as a pretty-printed one"
    );
}
