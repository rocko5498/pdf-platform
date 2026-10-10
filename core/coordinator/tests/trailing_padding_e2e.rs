//! A file with a long tail after `%%EOF` opens, and says it was repaired.
//! [FR-VIEW-2, SDS §10.4, GR-8]
//!
//! Padding added by a mail gateway, a wrapper or a download manager pushed
//! `startxref` out of the last 1024 bytes the specification names, and the
//! product refused the file ("no startxref marker found") while PDFium opened
//! it. Real worker, real IPC.
//!
//! The worker binary is not rebuilt by `cargo test -p coordinator`; build it
//! first (`cargo build -p worker-main`). [T-11]

mod common;

use common::{numbered_pdf, page_markers, scratch};

#[test]
fn a_file_with_a_long_tail_opens_with_all_its_pages() {
    let dir = scratch("trailing-padding");
    let path = dir.join("padded.pdf");
    let mut bytes = numbered_pdf(3);
    bytes.extend(std::iter::repeat(b' ').take(5000));
    std::fs::write(&path, &bytes).expect("write");

    assert_eq!(
        page_markers(&path),
        ["PAGEMARK1", "PAGEMARK2", "PAGEMARK3"],
        "every page of a padded file must be readable"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
