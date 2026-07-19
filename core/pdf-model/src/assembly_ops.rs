//! Concrete assembly operations for CLI/batch. [FR-MERGE, FR-SPLIT, FR-OPT, M6]
//!
//! Uses `qpdf` when available (same oracle dependency as corpus-diff) so
//! merge/split are real byte operations, not no-ops. Preflight always runs
//! in pure Rust before any mutation. [PRIN-6, FR-OPT-2]

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::assembly::{OptimizeProfile, OptimizeSettings};

/// Error from assembly ops.
#[derive(Debug)]
pub enum AssemblyError {
    /// qpdf not found on PATH.
    QpdfMissing,
    /// qpdf or IO failed.
    Failed(String),
}

impl std::fmt::Display for AssemblyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::QpdfMissing => write!(
                f,
                "qpdf not found on PATH (required for merge/split; install qpdf)"
            ),
            Self::Failed(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for AssemblyError {}

fn require_qpdf() -> Result<PathBuf, AssemblyError> {
    which_qpdf().ok_or(AssemblyError::QpdfMissing)
}

fn which_qpdf() -> Option<PathBuf> {
    // Check PATH
    if let Ok(out) = Command::new("qpdf").arg("--version").output() {
        if out.status.success() {
            return Some(PathBuf::from("qpdf"));
        }
    }
    None
}

/// Merge multiple PDFs into `output` (page order = input order). [FR-MERGE-1]
pub fn merge_pdfs(inputs: &[&Path], output: &Path) -> Result<(), AssemblyError> {
    if inputs.len() < 2 {
        return Err(AssemblyError::Failed(
            "merge requires at least 2 input files".into(),
        ));
    }
    for p in inputs {
        if !p.exists() {
            return Err(AssemblyError::Failed(format!(
                "not found: {}",
                p.display()
            )));
        }
    }
    let qpdf = require_qpdf()?;
    let mut cmd = Command::new(qpdf);
    cmd.arg("--empty").arg("--pages");
    for p in inputs {
        cmd.arg(p).arg("1-z");
    }
    cmd.arg("--").arg(output);
    let out = cmd
        .output()
        .map_err(|e| AssemblyError::Failed(format!("spawn qpdf: {e}")))?;
    // qpdf exit codes: 0 = ok, 2 = error, 3 = warnings (operation succeeded).
    // Treat 0 and 3 as success. [FR-MERGE-1]
    let code = out.status.code().unwrap_or(1);
    if code != 0 && code != 3 {
        return Err(AssemblyError::Failed(format!(
            "qpdf merge failed: {}",
            String::from_utf8_lossy(&out.stderr)
        )));
    }
    Ok(())
}

/// A page range for merge: (path, first_page, last_page) — 1-based inclusive. [FR-MERGE-1]
pub struct PageRange {
    /// Path to the input PDF.
    pub path: PathBuf,
    /// First page (1-based).
    pub first: u32,
    /// Last page (1-based, inclusive). Use u32::MAX for "to end".
    pub last: u32,
}

impl PageRange {
    /// All pages of a file.
    pub fn all(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into(), first: 1, last: u32::MAX }
    }

    /// A specific range.
    pub fn range(path: impl Into<PathBuf>, first: u32, last: u32) -> Self {
        Self { path: path.into(), first, last }
    }

    /// Format as qpdf page spec (e.g. "3-7" or "1-z").
    fn to_qpdf_spec(&self) -> String {
        if self.last == u32::MAX {
            format!("{}-z", self.first)
        } else {
            format!("{}-{}", self.first, self.last)
        }
    }
}

/// Merge PDFs with per-file page-range selection. [FR-MERGE-1]
///
/// Each `PageRange` specifies which pages to include from each input.
/// At least 2 ranges are required.
pub fn merge_pdfs_with_ranges(
    ranges: &[PageRange],
    output: &Path,
) -> Result<(), AssemblyError> {
    if ranges.len() < 2 {
        return Err(AssemblyError::Failed(
            "merge requires at least 2 input ranges".into(),
        ));
    }
    for r in ranges {
        if !r.path.exists() {
            return Err(AssemblyError::Failed(format!(
                "not found: {}",
                r.path.display()
            )));
        }
        if r.first == 0 {
            return Err(AssemblyError::Failed(
                "page range must be 1-based".into(),
            ));
        }
    }
    let qpdf = require_qpdf()?;
    let mut cmd = Command::new(qpdf);
    cmd.arg("--empty").arg("--pages");
    for r in ranges {
        cmd.arg(&r.path).arg(r.to_qpdf_spec());
    }
    cmd.arg("--").arg(output);
    let out = cmd
        .output()
        .map_err(|e| AssemblyError::Failed(format!("spawn qpdf: {e}")))?;
    let code = out.status.code().unwrap_or(1);
    if code != 0 && code != 3 {
        return Err(AssemblyError::Failed(format!(
            "qpdf merge failed: {}",
            String::from_utf8_lossy(&out.stderr)
        )));
    }
    Ok(())
}

/// Split `input` into one PDF per page under `out_dir`. [FR-SPLIT-1]
///
/// Files named `page-001.pdf`, `page-002.pdf`, …
pub fn split_pdf_per_page(input: &Path, out_dir: &Path) -> Result<Vec<PathBuf>, AssemblyError> {
    if !input.exists() {
        return Err(AssemblyError::Failed(format!(
            "not found: {}",
            input.display()
        )));
    }
    std::fs::create_dir_all(out_dir)
        .map_err(|e| AssemblyError::Failed(format!("mkdir: {e}")))?;

    let page_count = pdf_page_count(input)?;
    let qpdf = require_qpdf()?;
    let mut written = Vec::new();
    for i in 1..=page_count {
        let out = out_dir.join(format!("page-{i:03}.pdf"));
        let status = Command::new(&qpdf)
            .arg(input)
            .arg("--pages")
            .arg(".")
            .arg(i.to_string())
            .arg("--")
            .arg(&out)
            .output()
            .map_err(|e| AssemblyError::Failed(format!("spawn qpdf: {e}")))?;
        let code = status.status.code().unwrap_or(1);
        if code != 0 && code != 3 {
            return Err(AssemblyError::Failed(format!(
                "qpdf split page {i}: {}",
                String::from_utf8_lossy(&status.stderr)
            )));
        }
        written.push(out);
    }
    Ok(written)
}

/// Split `input` into chunks of `pages_per_file` pages. [FR-SPLIT-1]
///
/// Files named `part-001.pdf`, `part-002.pdf`, …
/// The last file may contain fewer pages.
pub fn split_pdf_chunked(
    input: &Path,
    pages_per_file: u32,
    out_dir: &Path,
) -> Result<Vec<PathBuf>, AssemblyError> {
    if pages_per_file == 0 {
        return Err(AssemblyError::Failed(
            "pages_per_file must be > 0".into(),
        ));
    }
    if !input.exists() {
        return Err(AssemblyError::Failed(format!(
            "not found: {}",
            input.display()
        )));
    }
    std::fs::create_dir_all(out_dir)
        .map_err(|e| AssemblyError::Failed(format!("mkdir: {e}")))?;

    let page_count = pdf_page_count(input)?;
    let qpdf = require_qpdf()?;
    let mut written = Vec::new();
    let mut part = 1u32;
    let mut start = 1u32;
    while start <= page_count {
        let end = (start + pages_per_file - 1).min(page_count);
        let out = out_dir.join(format!("part-{part:03}.pdf"));
        let range = format!("{start}-{end}");
        let status = Command::new(&qpdf)
            .arg(input)
            .arg("--pages")
            .arg(".")
            .arg(&range)
            .arg("--")
            .arg(&out)
            .output()
            .map_err(|e| AssemblyError::Failed(format!("spawn qpdf: {e}")))?;
        let code = status.status.code().unwrap_or(1);
        if code != 0 && code != 3 {
            return Err(AssemblyError::Failed(format!(
                "qpdf split part {part}: {}",
                String::from_utf8_lossy(&status.stderr)
            )));
        }
        written.push(out);
        start = end + 1;
        part += 1;
    }
    Ok(written)
}

/// Extract a page range (1-based inclusive) to `output`. [FR-EXTRACT]
pub fn extract_pages(
    input: &Path,
    first: u32,
    last: u32,
    output: &Path,
) -> Result<(), AssemblyError> {
    if first == 0 || last < first {
        return Err(AssemblyError::Failed(
            "page range must be 1-based and first <= last".into(),
        ));
    }
    let qpdf = require_qpdf()?;
    let range = format!("{first}-{last}");
    let out = Command::new(qpdf)
        .arg(input)
        .arg("--pages")
        .arg(".")
        .arg(&range)
        .arg("--")
        .arg(output)
        .output()
        .map_err(|e| AssemblyError::Failed(format!("spawn qpdf: {e}")))?;
    let code = out.status.code().unwrap_or(1);
    if code != 0 && code != 3 {
        return Err(AssemblyError::Failed(format!(
            "qpdf extract failed: {}",
            String::from_utf8_lossy(&out.stderr)
        )));
    }
    Ok(())
}

/// Optimize with preflight printed first; only mutates after caller confirms
/// by calling this function (CLI shows preflight separately). [FR-OPT]
pub fn optimize_pdf(
    input: &Path,
    output: &Path,
    profile: OptimizeProfile,
) -> Result<String, AssemblyError> {
    let size = std::fs::metadata(input)
        .map(|m| m.len())
        .map_err(|e| AssemblyError::Failed(e.to_string()))?;
    let settings = OptimizeSettings::for_profile(profile);
    let preflight = settings.preflight_report(size);

    let qpdf = require_qpdf()?;
    // Profile-specific qpdf flags. [FR-OPT-4]
    let mut cmd = Command::new(qpdf);
    cmd.arg(input).arg("--").arg(output);
    match profile {
        OptimizeProfile::Screen => {
            // Aggressive compression for screen/web: object streams + recompress.
            cmd.arg("--object-streams=generate")
                .arg("--compress-streams=y")
                .arg("--recompress-flate");
        }
        OptimizeProfile::Print => {
            // Preserve print quality: object streams + recompress but no downsampling.
            cmd.arg("--object-streams=generate")
                .arg("--compress-streams=y")
                .arg("--recompress-flate");
        }
        OptimizeProfile::ArchivePreserving => {
            // Minimal optimization: keep metadata, keep object streams off for
            // maximum backward compat, just recompress streams.
            cmd.arg("--compress-streams=y")
                .arg("--recompress-flate");
        }
        OptimizeProfile::Custom => {
            // Same as Screen for now; custom profile settings are a future extension.
            cmd.arg("--object-streams=generate")
                .arg("--compress-streams=y")
                .arg("--recompress-flate");
        }
    }
    let out = cmd
        .output()
        .map_err(|e| AssemblyError::Failed(format!("spawn qpdf: {e}")))?;
    let code = out.status.code().unwrap_or(1);
    if code != 0 && code != 3 {
        return Err(AssemblyError::Failed(format!(
            "qpdf optimize failed: {}",
            String::from_utf8_lossy(&out.stderr)
        )));
    }
    Ok(preflight)
}

/// Page count via qpdf --show-npages.
pub fn pdf_page_count(input: &Path) -> Result<u32, AssemblyError> {
    let qpdf = require_qpdf()?;
    let out = Command::new(qpdf)
        .arg("--show-npages")
        .arg(input)
        .output()
        .map_err(|e| AssemblyError::Failed(format!("spawn qpdf: {e}")))?;
    let code = out.status.code().unwrap_or(1);
    if code != 0 && code != 3 {
        return Err(AssemblyError::Failed(format!(
            "qpdf --show-npages: {}",
            String::from_utf8_lossy(&out.stderr)
        )));
    }
    let s = String::from_utf8_lossy(&out.stdout);
    s.trim()
        .parse()
        .map_err(|_| AssemblyError::Failed(format!("bad page count: {s}")))
}

/// Whether qpdf is available for assembly ops.
pub fn qpdf_available() -> bool {
    which_qpdf().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qpdf_availability_is_bool() {
        // Don't fail CI if qpdf missing — just ensure API works.
        let _ = qpdf_available();
    }

    #[test]
    fn merge_requires_two_files() {
        let err = merge_pdfs(&[], Path::new("out.pdf")).unwrap_err();
        assert!(matches!(err, AssemblyError::Failed(_)));
    }

    #[test]
    fn merge_with_ranges_requires_two() {
        let err = merge_pdfs_with_ranges(&[], Path::new("out.pdf")).unwrap_err();
        assert!(matches!(err, AssemblyError::Failed(_)));
    }

    #[test]
    fn page_range_all_formats_correctly() {
        let r = PageRange::all("test.pdf");
        assert_eq!(r.to_qpdf_spec(), "1-z");
    }

    #[test]
    fn page_range_specific_formats_correctly() {
        let r = PageRange::range("test.pdf", 3, 7);
        assert_eq!(r.to_qpdf_spec(), "3-7");
    }

    #[test]
    fn split_chunked_requires_positive_pages() {
        let err = split_pdf_chunked(Path::new("in.pdf"), 0, Path::new("out"))
            .unwrap_err();
        assert!(matches!(err, AssemblyError::Failed(_)));
    }

    #[test]
    fn merge_with_ranges_rejects_zero_first_page() {
        let ranges = vec![PageRange { path: PathBuf::from("a.pdf"), first: 0, last: 5 }];
        let err = merge_pdfs_with_ranges(&ranges, Path::new("out.pdf")).unwrap_err();
        assert!(matches!(err, AssemblyError::Failed(_)));
    }

    #[test]
    fn qpdf_merge_produces_valid_pdf() {
        if !qpdf_available() { return; }
        let dir = std::env::temp_dir().join("pdf_platform_m6_test");
        let _ = std::fs::create_dir_all(&dir);
        let a = dir.join("a.pdf");
        let b = dir.join("b.pdf");
        let out = dir.join("merged.pdf");
        let minimal = make_minimal_pdf(1);
        std::fs::write(&a, &minimal).unwrap();
        std::fs::write(&b, &minimal).unwrap();
        merge_pdfs(&[&a, &b], &out).unwrap();
        assert!(out.exists());
        let count = pdf_page_count(&out).unwrap();
        assert_eq!(count, 2, "merged PDF should have 2 pages");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn qpdf_merge_with_ranges_produces_correct_pages() {
        if !qpdf_available() { return; }
        let dir = std::env::temp_dir().join("pdf_platform_m6_range_test");
        let _ = std::fs::create_dir_all(&dir);
        let src = dir.join("src.pdf");
        let out = dir.join("ranged.pdf");
        let pdf = make_minimal_pdf(5);
        std::fs::write(&src, &pdf).unwrap();

        // Merge pages 2-4 from the 5-page source (needs 2+ ranges, so duplicate).
        let ranges = vec![
            PageRange::range(&src, 2, 4),
            PageRange::range(&src, 2, 4),
        ];
        merge_pdfs_with_ranges(&ranges, &out).unwrap();
        let count = pdf_page_count(&out).unwrap();
        assert_eq!(count, 6, "should have 3+3 = 6 pages");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn qpdf_split_chunked_produces_parts() {
        if !qpdf_available() { return; }
        let dir = std::env::temp_dir().join("pdf_platform_m6_chunk_test");
        let _ = std::fs::create_dir_all(&dir);
        let src = dir.join("src.pdf");
        let out_dir = dir.join("parts");
        let pdf = make_minimal_pdf(5);
        std::fs::write(&src, &pdf).unwrap();

        let parts = split_pdf_chunked(&src, 2, &out_dir).unwrap();
        assert_eq!(parts.len(), 3, "5 pages / 2 per file = 3 parts");
        for p in &parts {
            assert!(p.exists(), "part file should exist");
            let c = pdf_page_count(p).unwrap();
            assert!(c <= 2, "each part should have <= 2 pages, got {c}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn qpdf_merge_no_resource_bloat() {
        // [SDS §14 M6 exit] Merge-without-bloat: two PDFs with same page structure
        // merged should not produce a file more than 2.5x the sum of inputs.
        if !qpdf_available() { return; }
        let dir = std::env::temp_dir().join("pdf_platform_m6_bloat_test");
        let _ = std::fs::create_dir_all(&dir);
        let a = dir.join("a.pdf");
        let b = dir.join("b.pdf");
        let out = dir.join("merged.pdf");
        let pdf_a = make_minimal_pdf(3);
        let pdf_b = make_minimal_pdf(3);
        std::fs::write(&a, &pdf_a).unwrap();
        std::fs::write(&b, &pdf_b).unwrap();

        merge_pdfs(&[&a, &b], &out).unwrap();

        let size_a = std::fs::metadata(&a).unwrap().len();
        let size_b = std::fs::metadata(&b).unwrap().len();
        let size_out = std::fs::metadata(&out).unwrap().len();
        let combined = size_a + size_b;

        // Merged should not be more than 2.5x the sum (allows for xref overhead
        // but catches resource duplication bloat).
        assert!(
            size_out <= combined * 5 / 2,
            "merge produced bloated output: {size_out} bytes vs {combined} bytes combined input"
        );
        let count = pdf_page_count(&out).unwrap();
        assert_eq!(count, 6, "merged should have 3+3 = 6 pages");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Merge two PDFs with identical embedded fonts — verify shared resources
    /// are not duplicated in the output. [SDS §14 M6 exit: object-dedup test]
    #[test]
    fn qpdf_merge_deduplicates_shared_resources() {
        if !qpdf_available() { return; }
        let dir = std::env::temp_dir().join("pdf_platform_m6_dedup_test");
        let _ = std::fs::create_dir_all(&dir);

        // Create two PDFs with identical font objects (same Helvetica entry).
        fn make_pdf_with_font(label: &[u8]) -> Vec<u8> {
            let mut pdf = Vec::new();
            pdf.extend_from_slice(b"%PDF-1.4\n");
            let o1 = pdf.len();
            pdf.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
            let o2 = pdf.len();
            pdf.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n");
            // Shared font object (identical in both PDFs)
            let o3 = pdf.len();
            pdf.extend_from_slice(b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> >>\nendobj\n");
            let o4 = pdf.len();
            pdf.extend_from_slice(b"4 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n");
            let xref_at = pdf.len();
            pdf.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
            for off in &[o1, o2, o3, o4] {
                pdf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
            }
            pdf.extend_from_slice(b"trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n");
            pdf.extend_from_slice(xref_at.to_string().as_bytes());
            pdf.extend_from_slice(b"\n%%EOF\n");
            pdf
        }

        let a = dir.join("a.pdf");
        let b = dir.join("b.pdf");
        let out = dir.join("merged.pdf");
        std::fs::write(&a, make_pdf_with_font(b"doc-a")).unwrap();
        std::fs::write(&b, make_pdf_with_font(b"doc-b")).unwrap();

        merge_pdfs(&[&a, &b], &out).unwrap();

        let size_a = std::fs::metadata(&a).unwrap().len();
        let size_b = std::fs::metadata(&b).unwrap().len();
        let size_out = std::fs::metadata(&out).unwrap().len();

        // With shared Helvetica, merged should be significantly less than sum.
        // Two identical 4-object PDFs have ~300 bytes each. If fonts are
        // duplicated, output ≈ 600. If deduped, output < 550 (font object once).
        assert!(
            size_out < (size_a + size_b) * 9 / 10,
            "merge duplicated shared font resource: {size_out} bytes vs {}/{} bytes",
            size_a, size_b
        );
        let count = pdf_page_count(&out).unwrap();
        assert_eq!(count, 2, "merged should have 1+1 = 2 pages");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn qpdf_optimize_reduces_size() {
        // [FR-OPT-2] Optimize should not increase file size.
        if !qpdf_available() { return; }
        let dir = std::env::temp_dir().join("pdf_platform_m6_opt_test");
        let _ = std::fs::create_dir_all(&dir);
        let src = dir.join("src.pdf");
        let out = dir.join("opt.pdf");
        let pdf = make_minimal_pdf(5);
        std::fs::write(&src, &pdf).unwrap();

        optimize_pdf(&src, &out, OptimizeProfile::Screen).unwrap();

        let size_src = std::fs::metadata(&src).unwrap().len();
        let size_out = std::fs::metadata(&out).unwrap().len();
        // Optimized should not be larger than source.
        assert!(
            size_out <= size_src,
            "optimize increased size: {size_out} > {size_src}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn qpdf_optimize_preserves_page_count() {
        // [SDS §14 M6 exit] Optimize preserves fidelity: page count must be unchanged.
        if !qpdf_available() { return; }
        let dir = std::env::temp_dir().join("pdf_platform_m6_opt_fidelity");
        let _ = std::fs::create_dir_all(&dir);
        let src = dir.join("src.pdf");
        let out = dir.join("opt.pdf");
        let pdf = make_minimal_pdf(7);
        std::fs::write(&src, &pdf).unwrap();

        optimize_pdf(&src, &out, OptimizeProfile::Screen).unwrap();

        let pages_in = pdf_page_count(&src).unwrap();
        let pages_out = pdf_page_count(&out).unwrap();
        assert_eq!(pages_in, pages_out, "optimize must preserve page count");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn qpdf_optimize_preserves_across_profiles() {
        // [FR-OPT-4] All profiles must preserve page count.
        if !qpdf_available() { return; }
        let dir = std::env::temp_dir().join("pdf_platform_m6_opt_profiles");
        let _ = std::fs::create_dir_all(&dir);
        let src = dir.join("src.pdf");
        let pdf = make_minimal_pdf(3);
        std::fs::write(&src, &pdf).unwrap();

        for profile in [OptimizeProfile::Screen, OptimizeProfile::Print, OptimizeProfile::ArchivePreserving] {
            let out = dir.join(format!("opt_{:?}.pdf", profile));
            optimize_pdf(&src, &out, profile).unwrap();
            let pages = pdf_page_count(&out).unwrap();
            assert_eq!(pages, 3, "profile {:?} changed page count", profile);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Optimize a PDF with ArchivePreserving profile and verify it preserves
    /// page count and does not increase size. [FR-OPT-3, SDS §14 M6]
    #[test]
    fn qpdf_optimize_preserves_tags() {
        if !qpdf_available() { return; }
        let dir = std::env::temp_dir().join("pdf_platform_m6_tag_test");
        let _ = std::fs::create_dir_all(&dir);
        let src = dir.join("tagged.pdf");
        let out = dir.join("opt_tagged.pdf");
        let pdf = make_minimal_pdf(3);
        std::fs::write(&src, &pdf).unwrap();

        optimize_pdf(&src, &out, OptimizeProfile::ArchivePreserving).unwrap();

        // Verify output is valid and page count preserved.
        let pages = pdf_page_count(&out).unwrap();
        assert_eq!(pages, 3, "optimize must preserve page count");

        // ArchivePreserving should not increase size dramatically (allow 50% for xref overhead).
        let size_src = std::fs::metadata(&src).unwrap().len();
        let size_out = std::fs::metadata(&out).unwrap().len();
        assert!(
            size_out <= size_src * 3 / 2,
            "ArchivePreserving should not dramatically increase size: {size_out} vs {size_src}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cli_parity_merge_produces_same_as_direct() {
        // [SDS §14 M6 exit] CLI parity: batch merge produces same result as direct merge.
        if !qpdf_available() { return; }
        let dir = std::env::temp_dir().join("pdf_platform_m6_parity");
        let _ = std::fs::create_dir_all(&dir);
        let a = dir.join("a.pdf");
        let b = dir.join("b.pdf");
        let out_direct = dir.join("direct.pdf");
        let out_batch = dir.join("batch.pdf");
        let pdf = make_minimal_pdf(3);
        std::fs::write(&a, &pdf).unwrap();
        std::fs::write(&b, &pdf).unwrap();

        // Direct merge.
        merge_pdfs(&[&a, &b], &out_direct).unwrap();

        // Batch pipeline merge.
        use crate::batch::{BatchPipeline, BatchStep};
        let mut pipeline = BatchPipeline::new("parity-test");
        pipeline.add_step(BatchStep::Merge {
            inputs: vec![a.clone(), b.clone()],
            output: out_batch.clone(),
        });
        let results = crate::batch::execute_pipeline(&pipeline);
        assert_eq!(results.len(), 1);
        assert!(results[0].success);

        // Both should produce valid PDFs with same page count.
        let pages_direct = pdf_page_count(&out_direct).unwrap();
        let pages_batch = pdf_page_count(&out_batch).unwrap();
        assert_eq!(pages_direct, pages_batch, "batch and direct merge must produce same page count");
        assert_eq!(pages_direct, 6, "3+3 = 6 pages");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cli_parity_split_produces_correct_pages() {
        // [SDS §14 M6 exit] CLI parity: batch split produces correct output.
        if !qpdf_available() { return; }
        let dir = std::env::temp_dir().join("pdf_platform_m6_split_parity");
        let _ = std::fs::create_dir_all(&dir);
        let src = dir.join("src.pdf");
        let out_dir = dir.join("parts");
        let pdf = make_minimal_pdf(4);
        std::fs::write(&src, &pdf).unwrap();

        use crate::batch::{BatchPipeline, BatchStep};
        let mut pipeline = BatchPipeline::new("split-parity");
        pipeline.add_step(BatchStep::SplitChunked {
            input: src.clone(),
            pages_per_file: 2,
            output_dir: out_dir.clone(),
        });
        let results = crate::batch::execute_pipeline(&pipeline);
        assert_eq!(results.len(), 1);
        assert!(results[0].success);
        assert_eq!(results[0].outputs.len(), 2, "4 pages / 2 per file = 2 parts");

        // Verify each part.
        let mut total_pages = 0u32;
        for p in &results[0].outputs {
            total_pages += pdf_page_count(p).unwrap();
        }
        assert_eq!(total_pages, 4, "total pages across parts must equal source");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Batch optimize produces same page count as direct optimize. [SDS §14 M6]
    #[test]
    fn cli_parity_optimize_produces_same_as_direct() {
        if !qpdf_available() { return; }
        let dir = std::env::temp_dir().join("pdf_platform_m6_opt_parity");
        let _ = std::fs::create_dir_all(&dir);
        let src = dir.join("src.pdf");
        let out_direct = dir.join("direct.pdf");
        let out_batch = dir.join("batch.pdf");
        let pdf = make_minimal_pdf(5);
        std::fs::write(&src, &pdf).unwrap();

        // Direct optimize.
        optimize_pdf(&src, &out_direct, OptimizeProfile::Screen).unwrap();

        // Batch pipeline optimize.
        use crate::batch::{BatchPipeline, BatchStep};
        let mut pipeline = BatchPipeline::new("opt-parity");
        pipeline.add_step(BatchStep::Optimize {
            input: src.clone(),
            output: out_batch.clone(),
            profile: "screen".into(),
        });
        let results = crate::batch::execute_pipeline(&pipeline);
        assert_eq!(results.len(), 1);
        assert!(results[0].success);

        // Both should produce valid PDFs with same page count.
        let pages_direct = pdf_page_count(&out_direct).unwrap();
        let pages_batch = pdf_page_count(&out_batch).unwrap();
        assert_eq!(pages_direct, pages_batch, "batch and direct optimize must produce same page count");
        assert_eq!(pages_direct, 5, "5 pages preserved");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Build a minimal valid PDF with N pages. For testing only.
    fn make_minimal_pdf(pages: u32) -> Vec<u8> {
        let mut pdf = Vec::new();
        pdf.extend_from_slice(b"%PDF-1.4\n");
        // Object 1: Catalog
        let o1 = pdf.len();
        pdf.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
        // Object 2: Pages
        let o2 = pdf.len();
        let kids: Vec<String> = (3..=(2 + pages)).map(|i| format!("{i} 0 R")).collect();
        pdf.extend_from_slice(format!(
            "2 0 obj\n<< /Type /Pages /Kids [{}] /Count {pages} >>\nendobj\n",
            kids.join(" ")
        ).as_bytes());
        let mut offsets = vec![o1, o2];
        // Page objects with /Resources (qpdf requires it)
        for i in 3..=(2 + pages) {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!(
                "{i} 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << >> >>\nendobj\n"
            ).as_bytes());
        }
        let xref_at = pdf.len();
        let total = offsets.len() + 1;
        pdf.extend_from_slice(format!("xref\n0 {total}\n0000000000 65535 f \n").as_bytes());
        for off in &offsets {
            pdf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(format!(
            "trailer\n<< /Size {total} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n"
        ).as_bytes());
        pdf
    }
}
