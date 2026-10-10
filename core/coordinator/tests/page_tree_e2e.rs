//! Page operations on documents whose page tree is not flat, and on pages that
//! inherit their size and resources. [FR-ORG-1, FR-ROTATE, FR-STAMP, ADR-012]
//!
//! Three defects lived here, all invisible while every fixture was a flat tree
//! with a `/MediaBox` on each page:
//!
//! * `delete-pages --pages 2` on a nested document removed a whole *branch* of
//!   the tree while reporting one page deleted;
//! * the delete builder rewrote the `/Pages` node as `<< /Type /Pages /Kids
//!   [..] /Count n >>`, dropping the inherited `/MediaBox` and `/Resources`, so
//!   deleting one page changed the size of every other;
//! * rotate and stamp looked pages up in the root's `/Kids`, which in a nested
//!   tree are branches.
//!
//! Real worker, real IPC. The worker binary is not rebuilt by `cargo test -p
//! coordinator`; build it first (`cargo build -p worker-main`). [T-11]

mod common;

use common::{inherited_box_pdf, inherited_rotation_pdf, nested_numbered_pdf, page_markers, scratch, worker_path};
use coordinator::document::DocumentCoordinator;

fn write(dir: &std::path::Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).expect("write");
    path
}

#[test]
fn deleting_one_page_of_a_nested_document_removes_exactly_that_page() {
    let dir = scratch("tree-delete-one");
    let input = write(&dir, "in.pdf", &nested_numbered_pdf());
    let output = dir.join("out.pdf");

    let mut coord = DocumentCoordinator::open(&worker_path(), &input).expect("open");
    assert_eq!(coord.page_count(), 4);
    coord.delete_pages(&[1]).expect("delete page 2");
    coord.save_incremental(&output).expect("save");
    let _ = coord.close();

    assert_eq!(
        page_markers(&output),
        ["PAGEMARK1", "PAGEMARK3", "PAGEMARK4"],
        "page 2 must be the only page that went"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn emptying_one_branch_leaves_the_other_intact() {
    let dir = scratch("tree-delete-branch");
    let input = write(&dir, "in.pdf", &nested_numbered_pdf());
    let output = dir.join("out.pdf");

    let mut coord = DocumentCoordinator::open(&worker_path(), &input).expect("open");
    coord.delete_pages(&[0, 1]).expect("delete both pages of branch A");
    coord.save_incremental(&output).expect("save");
    let _ = coord.close();

    assert_eq!(page_markers(&output), ["PAGEMARK3", "PAGEMARK4"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn deleting_a_page_does_not_change_the_size_of_the_pages_that_inherit_it() {
    let dir = scratch("tree-inherited-box");
    let input = write(&dir, "in.pdf", &inherited_box_pdf());
    let output = dir.join("out.pdf");

    let mut before = DocumentCoordinator::open(&worker_path(), &input).expect("open");
    let size_before = before.render_page(1, 1.0).expect("render");
    let _ = before.close();
    assert_eq!((size_before.width, size_before.height), (300, 400), "the premise: size is inherited");

    let mut coord = DocumentCoordinator::open(&worker_path(), &input).expect("open");
    coord.delete_pages(&[0]).expect("delete page 1");
    coord.save_incremental(&output).expect("save");
    let _ = coord.close();

    let mut after = DocumentCoordinator::open(&worker_path(), &output).expect("reopen");
    assert_eq!(after.page_count(), 1);
    let kept = after.render_page(0, 1.0).expect("render");
    let _ = after.close();
    assert_eq!(
        (kept.width, kept.height),
        (300, 400),
        "the surviving page lost the size its /Pages node handed down"
    );
    assert_eq!(page_markers(&output), ["PAGEMARK2"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rotating_a_page_of_a_nested_document_turns_that_page() {
    let dir = scratch("tree-rotate");
    let input = write(&dir, "in.pdf", &nested_numbered_pdf());
    let output = dir.join("out.pdf");

    let mut coord = DocumentCoordinator::open(&worker_path(), &input).expect("open");
    coord.rotate_pages(&[2], 90).expect("rotate page 3");
    coord.save_incremental(&output).expect("save");
    let _ = coord.close();

    let mut reopened = DocumentCoordinator::open(&worker_path(), &output).expect("reopen");
    let rotations: Vec<u32> =
        reopened.summary().page_dimensions_f().iter().map(|(_, _, r)| *r).collect();
    let _ = reopened.close();
    assert_eq!(rotations, [0, 0, 90, 0], "page 3, in document order, and no other");
}

#[test]
fn stamping_a_nested_document_stamps_every_page() {
    let dir = scratch("tree-stamp");
    let input = write(&dir, "in.pdf", &nested_numbered_pdf());
    let output = dir.join("out.pdf");

    let mut coord = DocumentCoordinator::open(&worker_path(), &input).expect("open");
    coord
        .apply_stamp(&pdf_model::stamp::Stamp {
            text: "REVIEWED".into(),
            ..pdf_model::stamp::Stamp::default()
        })
        .expect("stamp a nested document");
    coord.save_incremental(&output).expect("save");
    let _ = coord.close();

    let mut reopened = DocumentCoordinator::open(&worker_path(), &output).expect("reopen");
    for page in 0..4 {
        let text: String = reopened
            .get_page_text(page)
            .expect("extract")
            .lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(text.contains("REVIEWED"), "page {} was not stamped: {text:?}", page + 1);
        assert!(text.contains(&format!("PAGEMARK{}", page + 1)), "page {} lost its text: {text:?}", page + 1);
    }
    let _ = reopened.close();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_ocr_page_lookup_resolves_the_real_page_in_a_nested_tree() {
    let dir = scratch("tree-page-object");
    let input = write(&dir, "in.pdf", &nested_numbered_pdf());

    let mut coord = DocumentCoordinator::open(&worker_path(), &input).expect("open");
    let (object, bytes) = coord.page_object(2).expect("page 3");
    let _ = coord.close();

    assert_eq!(object, 8, "the third page is object 8, the first page of branch B");
    assert!(String::from_utf8_lossy(&bytes).contains("/Type /Page "), "not a page: {:?}", String::from_utf8_lossy(&bytes));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rotating_a_page_that_inherits_its_rotation_adds_to_it() {
    let dir = scratch("tree-inherited-rotate");
    let input = write(&dir, "in.pdf", &inherited_rotation_pdf());
    let output = dir.join("out.pdf");

    let mut before = DocumentCoordinator::open(&worker_path(), &input).expect("open");
    let rotations_before: Vec<u32> = before.summary().page_dimensions_f().iter().map(|(_, _, r)| *r).collect();
    let _ = before.close();
    assert_eq!(rotations_before, [90, 90], "the premise: both pages inherit a quarter turn");

    let mut coord = DocumentCoordinator::open(&worker_path(), &input).expect("open");
    coord.rotate_pages(&[0], 90).expect("rotate page 1");
    coord.save_incremental(&output).expect("save");
    let _ = coord.close();

    let mut after = DocumentCoordinator::open(&worker_path(), &output).expect("reopen");
    let rotations: Vec<u32> = after.summary().page_dimensions_f().iter().map(|(_, _, r)| *r).collect();
    let _ = after.close();
    assert_eq!(rotations, [180, 90], "inherited 90 plus 90 is 180; the other page is untouched");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cropping_a_page_changes_that_pages_size_and_no_other() {
    let dir = scratch("tree-crop");
    let input = write(&dir, "in.pdf", &nested_numbered_pdf());
    let output = dir.join("out.pdf");

    let mut coord = DocumentCoordinator::open(&worker_path(), &input).expect("open");
    coord.crop_pages(&[2], [0.0, 0.0, 200.0, 300.0]).expect("crop page 3");
    coord.save_incremental(&output).expect("save");
    let _ = coord.close();

    let mut reopened = DocumentCoordinator::open(&worker_path(), &output).expect("reopen");
    let sizes: Vec<(u32, u32)> = (0..4)
        .map(|p| {
            let r = reopened.render_page(p, 1.0).expect("render");
            (r.width, r.height)
        })
        .collect();
    let _ = reopened.close();
    assert_eq!(sizes, [(612, 792), (612, 792), (200, 300), (612, 792)], "only page 3 is cropped");
    assert_eq!(page_markers(&output), ["PAGEMARK1", "PAGEMARK2", "PAGEMARK3", "PAGEMARK4"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_crop_is_checked_against_the_media_box_the_page_inherits() {
    let dir = scratch("tree-crop-inherited");
    let input = write(&dir, "in.pdf", &inherited_box_pdf());

    let mut coord = DocumentCoordinator::open(&worker_path(), &input).expect("open");
    // 300 x 400 is inherited from the /Pages node; this box fits a letter page
    // and not this one.
    let error = coord.crop_pages(&[0], [0.0, 0.0, 600.0, 700.0]).unwrap_err().to_string();
    assert!(error.contains("media box"), "{error}");
    assert!(coord.crop_pages(&[0], [10.0, 10.0, 290.0, 390.0]).is_ok(), "a box inside the inherited one is fine");
    let _ = coord.close();
    let _ = std::fs::remove_dir_all(&dir);
}
