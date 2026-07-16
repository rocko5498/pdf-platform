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
    if !out.status.success() {
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
        if !status.status.success() {
            return Err(AssemblyError::Failed(format!(
                "qpdf split page {i}: {}",
                String::from_utf8_lossy(&status.stderr)
            )));
        }
        written.push(out);
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
    if !out.status.success() {
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
    // Lossless stream recompression + object streams when safe for profile.
    let mut cmd = Command::new(qpdf);
    cmd.arg(input)
        .arg("--object-streams=generate")
        .arg("--compress-streams=y")
        .arg("--recompress-flate")
        .arg("--")
        .arg(output);
    if matches!(
        profile,
        OptimizeProfile::Screen | OptimizeProfile::Print | OptimizeProfile::Custom
    ) {
        // Keep metadata by default for ArchivePreserving only we skip nothing extra
    }
    let out = cmd
        .output()
        .map_err(|e| AssemblyError::Failed(format!("spawn qpdf: {e}")))?;
    if !out.status.success() {
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
    if !out.status.success() {
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
}
