//! Shared fixtures for coordinator end-to-end tests.
//!
//! Every page of the generated document says which page it is, so a test can
//! read the output back through the engine and check *which* pages survived an
//! operation rather than only how many.

#![allow(dead_code)] // each test binary uses a different subset

use std::path::{Path, PathBuf};

use coordinator::document::DocumentCoordinator;

/// The worker binary, resolved next to the test executable's build directory.
pub fn worker_path() -> PathBuf {
    let exe = std::env::current_exe().expect("current exe");
    let mut dir = exe.parent().expect("exe dir").to_path_buf();
    if dir.ends_with("deps") {
        dir.pop();
    }
    dir.join(if cfg!(windows) { "worker.exe" } else { "worker" })
}

/// A document of `pages` pages, each drawing `PAGEMARK<n>`.
pub fn numbered_pdf(pages: u32) -> Vec<u8> {
    let mut objects: Vec<Vec<u8>> = Vec::new();
    objects.push(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());

    let kids: Vec<String> = (0..pages).map(|i| format!("{} 0 R", 4 + i * 2)).collect();
    objects.push(
        format!("<< /Type /Pages /Kids [{}] /Count {pages} >>", kids.join(" ")).into_bytes(),
    );
    objects.push(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec());

    for index in 0..pages {
        let content_obj = 5 + index * 2;
        objects.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
                 /Resources << /Font << /F1 3 0 R >> >> /Contents {content_obj} 0 R >>"
            )
            .into_bytes(),
        );
        let content = format!("BT\n/F1 24 Tf\n72 700 Td\n(PAGEMARK{}) Tj\nET\n", index + 1);
        objects.push(
            [
                format!("<< /Length {} >>\nstream\n", content.len()).as_bytes(),
                content.as_bytes(),
                b"endstream",
            ]
            .concat(),
        );
    }

    assemble(&objects)
}

/// Serialize objects into a document with a correct xref table.
pub fn assemble(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes: Vec<u8> = b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        bytes.extend_from_slice(body);
        bytes.extend_from_slice(b"\nendobj\n");
    }
    let xref_at = bytes.len();
    bytes.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
    bytes.extend_from_slice(b"0000000000 65535 f \n");
    for offset in &offsets {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    bytes
}

/// The `PAGEMARK` each page carries, in document order.
pub fn page_markers(path: &Path) -> Vec<String> {
    let mut coord = DocumentCoordinator::open(&worker_path(), path)
        .unwrap_or_else(|error| panic!("open {}: {error}", path.display()));
    let count = coord.page_count();
    let mut markers = Vec::new();
    for index in 0..count {
        let model = coord
            .get_page_text(index)
            .unwrap_or_else(|error| panic!("extract page {index}: {error}"));
        let text: String = model.lines.iter().map(|line| line.text.as_str()).collect();
        markers.push(
            text.split_whitespace()
                .find(|token| token.starts_with("PAGEMARK"))
                .unwrap_or("<none>")
                .to_owned(),
        );
    }
    let _ = coord.close();
    markers
}

/// A scratch directory, emptied first.
pub fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pdf-platform-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}
