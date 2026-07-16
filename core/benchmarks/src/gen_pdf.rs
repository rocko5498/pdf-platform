//! Minimal multi-page PDF generator for benchmarks. [ADR-023, SDS §14 M1]
//!
//! Generates valid PDFs with N pages (classic xref, US Letter, no content streams)
//! for measuring large-document render pipeline performance.

use std::io::Write;

/// Generate a minimal N-page PDF as bytes.
///
/// Each page is US Letter (612×792 pt) with no content streams — just the
/// structural skeleton needed for the render pipeline to process page geometry.
pub fn generate_pdf_bytes(num_pages: u32) -> Vec<u8> {
    let mut buf = Vec::with_capacity(256 + num_pages as usize * 80);
    let mut offsets = Vec::with_capacity(num_pages as usize + 3);

    // Header
    buf.extend_from_slice(b"%PDF-1.4\n");

    // Object 1: Catalog
    offsets.push(buf.len() as u32);
    buf.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");

    // Object 2: Pages (parent)
    offsets.push(buf.len() as u32);
    let kids: Vec<String> = (0..num_pages)
        .map(|i| format!("{} 0 R", i + 3))
        .collect();
    write!(
        buf,
        "2 0 obj\n<< /Type /Pages /Kids [{}] /Count {} >>\nendobj\n",
        kids.join(" "),
        num_pages
    )
    .unwrap();

    // Objects 3..N+2: Page objects
    for i in 0..num_pages {
        offsets.push(buf.len() as u32);
        write!(
            buf,
            "{} 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>\nendobj\n",
            i + 3
        )
        .unwrap();
    }

    // xref table
    let xref_offset = buf.len() as u32;
    write!(&mut buf, "xref\n0 {}\n", num_pages + 3).unwrap();
    buf.extend_from_slice(b"0000000000 65535 f \n");
    for offset in &offsets {
        write!(&mut buf, "{:010} 00000 n \n", offset).unwrap();
    }

    // Trailer
    write!(
        buf,
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
        num_pages + 3,
        xref_offset
    )
    .unwrap();

    buf
}

/// Generate a multi-page PDF and write it to a temp file, returning the path.
pub fn generate_temp_pdf(num_pages: u32, label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("pdf-platform-bench-{}-{}p.pdf", label, num_pages));
    let bytes = generate_pdf_bytes(num_pages);
    std::fs::write(&path, &bytes).expect("write temp PDF");
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_1page_valid_pdf() {
        let bytes = generate_pdf_bytes(1);
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.starts_with("%PDF-1.4"));
        assert!(text.contains("1 0 obj"));
        assert!(text.contains("/Count 1"));
        assert!(text.contains("%%EOF"));
    }

    #[test]
    fn generate_100page_pdf() {
        let bytes = generate_pdf_bytes(100);
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("/Count 100"));
        assert!(text.contains("102 0 obj"));
    }

    #[test]
    fn temp_file_created() {
        let path = generate_temp_pdf(5, "test");
        assert!(path.exists());
        let meta = std::fs::metadata(&path).unwrap();
        assert!(meta.len() > 100);
        std::fs::remove_file(&path).ok();
    }
}
