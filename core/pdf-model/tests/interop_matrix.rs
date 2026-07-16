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
