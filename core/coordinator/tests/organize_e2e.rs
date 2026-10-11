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

/// A page with hyperlinks can be duplicated, and the copy gets links of its own
/// instead of sharing the original's. Found on real resumes, which are full of
/// links and could not be duplicated at all. [FR-ORG-1, T-12]
#[test]
fn duplicating_a_page_with_links_copies_the_links() {
    if !common::qpdf_or_skip() {
        return;
    }
    let dir = scratch("organize-duplicate-links");
    let source = dir.join("links.pdf");
    let link = |n: u32| format!("<< /Type /Annot /Subtype /Link /Rect [0 0 {n} {n}] /Border [0 0 0] /A << /S /URI /URI (https://example.com/{n}) >> >>");
    std::fs::write(
        &source,
        common::assemble(&[
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << >> /Annots [4 0 R 5 0 R] >>".to_vec(),
            link(10).into_bytes(),
            link(20).into_bytes(),
        ]),
    )
    .expect("write");

    let mut coord = DocumentCoordinator::open(&worker_path(), &source).expect("open");
    coord.duplicate_pages(&[0]).expect("duplicate a page that has links");
    let saved = dir.join("out.pdf");
    coord.save_incremental(&saved).expect("save");
    let _ = coord.close();

    let check = std::process::Command::new("qpdf").arg("--check").arg(&saved).output().expect("qpdf");
    assert!(matches!(check.status.code(), Some(0) | Some(3)), "{}", String::from_utf8_lossy(&check.stderr));
    let qdf = dir.join("out.qdf");
    let status = std::process::Command::new("qpdf")
        .args(["--qdf", "--object-streams=disable"])
        .arg(&saved)
        .arg(&qdf)
        .status()
        .expect("qpdf --qdf");
    assert!(matches!(status.code(), Some(0) | Some(3)));
    let text = String::from_utf8_lossy(&std::fs::read(&qdf).unwrap()).into_owned();
    assert_eq!(text.matches("/Subtype /Link").count(), 4, "two pages with two links each, none shared");
    let pages = std::process::Command::new("qpdf").arg("--show-npages").arg(&saved).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&pages.stdout).trim(), "2");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A source whose `startxref` points past the end of the file (a truncated
/// download) is rebuilt on open. Saving must then write a complete table; an
/// update chained to the bogus offset leaves a file that knows only the new
/// objects. Found by the pdf.js corpus edit sweep. [FR-SAVE, SDS §10.4, T-12]
#[test]
fn saving_a_file_whose_startxref_is_bogus_keeps_every_object_reachable() {
    if !common::qpdf_or_skip() {
        return;
    }
    let dir = scratch("organize-bogus-startxref");
    // A reduced test file as found in the pdf.js corpus: the cross-reference
    // table is still there but describes a larger original (every offset is
    // wrong for this file), and `startxref` points past the end.
    let full = numbered_pdf(3);
    // "\nxref\n", not "xref\n": the latter also matches inside "startxref\n".
    let table = 1 + full.windows(6).rposition(|w| w == b"\nxref\n").expect("xref table");
    let text = String::from_utf8_lossy(&full[table..]).into_owned();
    let mut stale = String::new();
    for line in text.lines() {
        if line.ends_with(" 00000 n ") || line.ends_with(" 00000 n") {
            let offset: u32 = line[..10].parse().expect("offset");
            stale.push_str(&format!("{:010} 00000 n \n", offset + 1000));
        } else if line.starts_with("startxref") {
            stale.push_str("startxref\n9999999\n");
            break;
        } else {
            stale.push_str(line);
            stale.push('\n');
        }
    }
    stale.push_str("%%EOF\n");
    let mut bytes = full[..table].to_vec();
    bytes.extend_from_slice(stale.as_bytes());
    let source = dir.join("bogus.pdf");
    std::fs::write(&source, &bytes).expect("write");

    let mut coord = DocumentCoordinator::open(&worker_path(), &source).expect("open a file that needs repair");
    coord.insert_blank_page(0, None).expect("insert");
    let saved = dir.join("out.pdf");
    coord.save_incremental(&saved).expect("save");
    let _ = coord.close();

    // The artefact: the update's own trailer must not chain to the bogus
    // offset, and its table must list the source's objects as well as the new
    // one, because there is no earlier section to inherit them from.
    let out = String::from_utf8_lossy(&std::fs::read(&saved).unwrap()).into_owned();
    let last_xref = out.rfind("\nxref\n").expect("an appended xref section");
    let appended = &out[last_xref..];
    assert!(!appended.contains("/Prev"), "chained to the damaged source's startxref: {appended}");
    assert!(appended.matches(" 00000 n").count() >= 4, "the table is not complete: {appended}");

    let pages = std::process::Command::new("qpdf").arg("--show-npages").arg(&saved).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&pages.stdout).trim(), "4", "qpdf does not see all four pages");
    let mut reopened = DocumentCoordinator::open(&worker_path(), &saved).expect("reopen");
    assert_eq!(reopened.page_count(), 4);
    let _ = reopened.close();
    let _ = std::fs::remove_dir_all(&dir);
}
