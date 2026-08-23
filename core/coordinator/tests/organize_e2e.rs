//! Page organize changes the document, not just the overlay. [FR-ORG-1, ADR-012]
//!
//! M3 names page organize as "the simplest real edit to exercise [the write
//! spine] end to end". What existed was an in-memory test: it builds a command
//! group against a hand-written `/Kids` string, applies it to a `CowOverlay`,
//! and asserts the string changed. No document is opened, nothing is saved, and
//! nothing reads the result back — so a save that dropped the change, or wrote
//! it somewhere a reader ignores, would pass.
//!
//! These delete and rotate pages of a real document, save, and reopen.

mod common;

use common::{numbered_pdf, page_markers, scratch, worker_path};
use coordinator::document::DocumentCoordinator;

#[test]
fn deleting_a_page_removes_that_page_from_the_saved_document() {
    let dir = scratch("organize-delete");
    let source = dir.join("source.pdf");
    std::fs::write(&source, numbered_pdf(5)).expect("write source");

    assert_eq!(
        page_markers(&source),
        vec![
            "PAGEMARK1", "PAGEMARK2", "PAGEMARK3", "PAGEMARK4", "PAGEMARK5"
        ],
        "the fixture must start with five identifiable pages"
    );

    let mut coord = DocumentCoordinator::open(&worker_path(), &source).expect("open");
    // Zero-based: page 2 of five, the one that says PAGEMARK2.
    coord.delete_pages(&[1]).expect("delete page 2");
    let saved = dir.join("deleted.pdf");
    coord.save_incremental(&saved).expect("save");
    let _ = coord.close();

    assert_eq!(
        page_markers(&saved),
        vec!["PAGEMARK1", "PAGEMARK3", "PAGEMARK4", "PAGEMARK5"],
        "the wrong page was removed, or the deletion did not reach the file"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn deleting_several_pages_removes_exactly_those_pages() {
    // Deleting more than one at a time is where index shifting bites: removing
    // page 1 renumbers everything after it, so a loop that deletes by original
    // index removes the wrong pages.
    let dir = scratch("organize-delete-many");
    let source = dir.join("source.pdf");
    std::fs::write(&source, numbered_pdf(5)).expect("write source");

    let mut coord = DocumentCoordinator::open(&worker_path(), &source).expect("open");
    coord.delete_pages(&[0, 2, 4]).expect("delete pages 1, 3 and 5");
    let saved = dir.join("deleted-many.pdf");
    coord.save_incremental(&saved).expect("save");
    let _ = coord.close();

    assert_eq!(
        page_markers(&saved),
        vec!["PAGEMARK2", "PAGEMARK4"],
        "deleting several pages removed the wrong ones"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rotating_a_page_is_visible_in_the_rendered_output() {
    // `/Rotate 90` in the page object is not the claim; the claim is that the
    // page comes out rotated. A quarter turn swaps the rendered page's width
    // and height, which no amount of dictionary inspection would confirm.
    // [FR-ORG-2, FR-CMP-1]
    let dir = scratch("organize-rotate");
    let source = dir.join("source.pdf");
    std::fs::write(&source, numbered_pdf(2)).expect("write source");

    let mut before = DocumentCoordinator::open(&worker_path(), &source).expect("open");
    let upright = before.render_page(0, 0.25).expect("render before");
    before.rotate_pages(&[0], 90).expect("rotate page 1");
    let saved = dir.join("rotated.pdf");
    before.save_incremental(&saved).expect("save");
    let _ = before.close();

    let mut after = DocumentCoordinator::open(&worker_path(), &saved).expect("reopen rotated");
    let turned = after.render_page(0, 0.25).expect("render after");
    let _ = after.close();

    assert_eq!(
        (turned.width, turned.height),
        (upright.height, upright.width),
        "a quarter turn must swap the rendered page's dimensions; before {}x{}, after {}x{}",
        upright.width,
        upright.height,
        turned.width,
        turned.height
    );

    // The untouched page must stay untouched.
    let mut after = DocumentCoordinator::open(&worker_path(), &saved).expect("reopen rotated");
    let second = after.render_page(1, 0.25).expect("render page 2");
    let _ = after.close();
    assert_eq!(
        (second.width, second.height),
        (upright.width, upright.height),
        "rotating one page changed another"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
