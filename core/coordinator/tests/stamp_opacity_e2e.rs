//! A translucent stamp must reach the rendered page. [FR-STAMP, FR-CMP-1]
//!
//! The stamp content stream used to set opacity by writing `{opacity} gs`. The
//! `gs` operator takes the *name* of an ExtGState resource, not a number, so
//! the operator was malformed and the resource it should have named did not
//! exist. The only test asserted the stream contained `gs`, which that
//! malformed operator satisfies.
//!
//! This drives the whole path — coordinator command group, incremental save,
//! reopen, render through the sandboxed worker — and requires the page's
//! pixels to change. A reader that rejects the operator, or a page whose
//! resources never gained the state the operator names, renders the original
//! page and fails here.

mod common;

use common::{numbered_pdf, scratch, worker_path};
use coordinator::document::DocumentCoordinator;
use pdf_model::stamp::{Stamp, StampPosition};

fn translucent_stamp() -> Stamp {
    Stamp {
        text: "DRAFT".into(),
        position: StampPosition::Center,
        font_size: 48.0,
        opacity: 0.35,
        ..Stamp::default()
    }
}

/// Fraction of pixels that differ between two same-sized rasters.
fn changed_fraction(before: &[u8], after: &[u8]) -> f32 {
    assert_eq!(before.len(), after.len(), "rasters differ in size");
    let differing = before
        .chunks_exact(4)
        .zip(after.chunks_exact(4))
        .filter(|(a, b)| a != b)
        .count();
    differing as f32 / (before.len() / 4) as f32
}

#[test]
fn a_translucent_stamp_is_visible_on_the_rendered_page() {
    let dir = scratch("stamp-opacity");
    let source = dir.join("source.pdf");
    std::fs::write(&source, numbered_pdf(1)).expect("write source");

    let mut coord = DocumentCoordinator::open(&worker_path(), &source).expect("open");
    let before = coord.render_page(0, 0.5).expect("render before");
    coord.apply_stamp(&translucent_stamp()).expect("stamp");
    let stamped = dir.join("stamped.pdf");
    coord.save_incremental(&stamped).expect("save");
    let _ = coord.close();

    let mut reopened = DocumentCoordinator::open(&worker_path(), &stamped).expect("reopen");
    let after = reopened.render_page(0, 0.5).expect("render after");
    let _ = reopened.close();

    assert_eq!(
        (before.width, before.height),
        (after.width, after.height),
        "stamping must not resize the page"
    );
    let changed = changed_fraction(&before.pixels, &after.pixels);
    assert!(
        changed > 0.001,
        "a translucent stamp changed {changed:.4} of the page's pixels: the \
         content stream did not draw"
    );
}

#[test]
fn the_stamped_page_declares_the_graphics_state_its_stream_names() {
    // The stream and the page resources are written by different functions.
    // If they disagree about the name — as the stream and `/Font` once did —
    // the operator refers to nothing and a strict reader drops it.
    let dir = scratch("stamp-opacity-resources");
    let source = dir.join("source.pdf");
    std::fs::write(&source, numbered_pdf(1)).expect("write source");

    let mut coord = DocumentCoordinator::open(&worker_path(), &source).expect("open");
    coord.apply_stamp(&translucent_stamp()).expect("stamp");
    let stamped = dir.join("stamped.pdf");
    coord.save_incremental(&stamped).expect("save");
    let _ = coord.close();

    let bytes = std::fs::read(&stamped).expect("read stamped");
    let text = String::from_utf8_lossy(&bytes);
    let name = pdf_model::stamp::EXT_GSTATE_NAME;

    assert!(
        text.contains(&format!("/{name} gs")),
        "the content stream must name the graphics state"
    );
    assert!(
        text.contains(&format!("/{name} << /Type /ExtGState")),
        "the page resources must define the state the stream names"
    );
    assert!(
        text.contains("/ca 0.350") && text.contains("/CA 0.350"),
        "the state must carry the stamp's opacity for fills and strokes"
    );
}

#[test]
fn an_opaque_stamp_adds_no_transparency_state() {
    // Opacity 1.0 needs no ExtGState; emitting one anyway would put a resource
    // into every stamped page for nothing.
    let dir = scratch("stamp-opaque");
    let source = dir.join("source.pdf");
    std::fs::write(&source, numbered_pdf(1)).expect("write source");

    let mut coord = DocumentCoordinator::open(&worker_path(), &source).expect("open");
    coord
        .apply_stamp(&Stamp {
            text: "FINAL".into(),
            ..Stamp::default()
        })
        .expect("stamp");
    let stamped = dir.join("opaque.pdf");
    coord.save_incremental(&stamped).expect("save");
    let _ = coord.close();

    let text = String::from_utf8_lossy(&std::fs::read(&stamped).expect("read")).to_string();
    assert!(!text.contains("/ExtGState"), "opaque stamp added a graphics state");
    assert!(!text.contains(" gs\n"), "opaque stamp emitted a `gs` operator");
}
