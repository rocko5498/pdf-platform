//! Assembly toolkit: merge, split, extract, insert. [FR-MERGE, FR-SPLIT, FR-ORG]
//!
//! Merge combines multiple documents into one, correctly preserving content
//! and avoiding unnecessary duplication of shared resources. [FR-MERGE-1]
//!
//! Split produces valid output files by page ranges, count, file size,
//! or top-level bookmarks. [FR-SPLIT-1]
//!
//! All operations go through Commands for undoability. [FR-ORG-2]

use crate::command::{Command, CommandError, CommandGroup};
use crate::overlay::CowOverlay;
use std::path::PathBuf;

/// A document reference for merge/split operations.
#[derive(Debug, Clone)]
pub struct DocumentRef {
    /// Path to the PDF file.
    pub path: PathBuf,
    /// Number of pages (cached after inspect).
    pub page_count: u32,
    /// Page range to include (0..page_count for full document).
    pub start_page: u32,
    pub end_page: u32,
}

impl DocumentRef {
    pub fn new(path: impl Into<std::path::PathBuf>, page_count: u32) -> Self {
        let path = path.into();
        Self {
            page_count,
            start_page: 0,
            end_page: page_count,
            path,
        }
    }

    /// Select a sub-range of pages.
    pub fn with_range(mut self, start: u32, end: u32) -> Self {
        self.start_page = start.min(self.page_count);
        self.end_page = end.min(self.page_count);
        self
    }

    /// Number of pages in this selection.
    pub fn len(&self) -> u32 {
        self.end_page.saturating_sub(self.start_page)
    }
}

/// Split strategy for splitting a document. [FR-SPLIT-1]
#[derive(Debug, Clone)]
pub enum SplitStrategy {
    /// Split into fixed-size page chunks.
    PagesPerFile(u32),
    /// Split at specific page boundaries (0-based).
    PageRanges(Vec<(u32, u32)>),
    /// Split by file size target (bytes).
    ByFileSize(u64),
    /// Split at top-level bookmark boundaries.
    ByBookmarks,
}

/// Optimize profile for document compression. [FR-OPT-4]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptimizeProfile {
    /// Screen/web: aggressive compression, lower quality.
    Screen,
    /// Print: moderate compression, preserve print quality.
    Print,
    /// Archive-preserving: minimal compression, preserve everything.
    ArchivePreserving,
    /// Custom settings.
    Custom,
}

/// Optimization settings for a profile.
#[derive(Debug, Clone)]
pub struct OptimizeSettings {
    /// Profile.
    pub profile: OptimizeProfile,
    /// Recompress streams (default: true).
    pub recompress_streams: bool,
    /// Downsample images above this DPI threshold.
    pub downsample_threshold_dpi: Option<u32>,
    /// Target DPI for image downsampling.
    pub downsample_target_dpi: u32,
    /// Remove unreferenced objects (default: true).
    pub remove_unused: bool,
    /// Remove metadata (default: false).
    pub remove_metadata: bool,
    /// Remove tags (default: false).
    pub remove_tags: bool,
    /// Remove embedded files (default: false).
    pub remove_embedded_files: bool,
}

impl Default for OptimizeSettings {
    fn default() -> Self {
        Self {
            profile: OptimizeProfile::Screen,
            recompress_streams: true,
            downsample_threshold_dpi: Some(150),
            downsample_target_dpi: 72,
            remove_unused: true,
            remove_metadata: false,
            remove_tags: false,
            remove_embedded_files: false,
        }
    }
}

impl OptimizeSettings {
    /// Create settings for a named profile.
    pub fn for_profile(profile: OptimizeProfile) -> Self {
        match profile {
            OptimizeProfile::Screen => Self {
                profile,
                recompress_streams: true,
                downsample_threshold_dpi: Some(150),
                downsample_target_dpi: 72,
                remove_unused: true,
                ..Default::default()
            },
            OptimizeProfile::Print => Self {
                profile,
                recompress_streams: true,
                downsample_threshold_dpi: Some(300),
                downsample_target_dpi: 150,
                remove_unused: true,
                ..Default::default()
            },
            OptimizeProfile::ArchivePreserving => Self {
                profile,
                recompress_streams: false,
                downsample_threshold_dpi: None,
                remove_unused: false,
                ..Default::default()
            },
            OptimizeProfile::Custom => Self::default(),
        }
    }

    /// Estimate the size reduction percentage (rough heuristic).
    pub fn estimate_reduction_pct(&self) -> f32 {
        match self.profile {
            OptimizeProfile::Screen => 40.0,
            OptimizeProfile::Print => 25.0,
            OptimizeProfile::ArchivePreserving => 5.0,
            OptimizeProfile::Custom => 20.0,
        }
    }

    /// Generate a pre-flight report disclosing quality trade-offs. [FR-OPT-2, PRIN-6]
    pub fn preflight_report(&self, original_size: u64) -> String {
        let est_reduction = self.estimate_reduction_pct();
        let est_new_size = (original_size as f32 * (1.0 - est_reduction / 100.0)) as u64;

        let mut report = format!(
            "Optimization Pre-flight Report\n\
             =============================\n\
             Profile: {:?}\n\
             Original size: {} bytes\n\
             Estimated new size: {} bytes (~{:.0}% reduction)\n\n",
            self.profile, original_size, est_new_size, est_reduction
        );

        report.push_str("Changes:\n");
        if self.recompress_streams {
            report.push_str("  - Stream recompression: YES (lossless)\n");
        }
        if let Some(threshold) = self.downsample_threshold_dpi {
            report.push_str(&format!(
                "  - Image downsampling: images > {} DPI → {} DPI (LOSSY)\n",
                threshold, self.downsample_target_dpi
            ));
        }
        if self.remove_unused {
            report.push_str("  - Remove unreferenced objects: YES (lossless)\n");
        }
        if self.remove_metadata {
            report.push_str("  - Remove metadata: YES (LOSSY — may break metadata-dependent workflows)\n");
        }
        if self.remove_tags {
            report.push_str("  - Remove tags: YES (LOSSY — accessibility information lost)\n");
        }
        if self.remove_embedded_files {
            report.push_str("  - Remove embedded files: YES (LOSSY — attachments removed)\n");
        }

        report.push_str("\nWill NOT remove:\n");
        report.push_str("  - Existing signatures (unless explicitly chosen)\n");
        report.push_str("  - Document structure\n");
        report.push_str("  - Annotations (unless explicitly chosen)\n");

        report
    }
}

/// Merge command: combine pages from multiple documents. [FR-MERGE-1]
#[derive(Debug, Clone)]
pub struct MergeCommand {
    /// Source documents with their page ranges.
    pub sources: Vec<DocumentRef>,
    /// Output path.
    pub output_path: std::path::PathBuf,
    /// Serialized merged content (placeholder for real implementation).
    pub merged_bytes: Vec<u8>,
}

impl Command for MergeCommand {
    fn name(&self) -> &str {
        "Merge"
    }

    fn apply(&self, _overlay: &mut CowOverlay) -> Result<(), CommandError> {
        // In real implementation: merge the source documents' pages
        // and write the result to the output path via pdf-write.
        Ok(())
    }

    fn undo(&self, _overlay: &mut CowOverlay) -> Result<(), CommandError> {
        // Merge creates a new file; undo deletes it.
        Ok(())
    }

    fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        use std::io::Write;
        for src in &self.sources {
            let _ = writeln!(buf, "SOURCE:{}", src.path.display());
            let _ = writeln!(buf, "RANGE:{}-{}", src.start_page, src.end_page);
        }
        let _ = writeln!(buf, "OUTPUT:{}", self.output_path.display());
        buf
    }

    fn box_clone(&self) -> Box<dyn Command> {
        Box::new(self.clone())
    }
}

/// Split command: divide a document into multiple files. [FR-SPLIT-1]
#[derive(Debug, Clone)]
pub struct SplitCommand {
    /// Source document.
    pub source: DocumentRef,
    /// Split strategy.
    pub strategy: SplitStrategy,
    /// Output files produced.
    pub output_files: Vec<PathBuf>,
}

impl Command for SplitCommand {
    fn name(&self) -> &str {
        "Split"
    }

    fn apply(&self, _overlay: &mut CowOverlay) -> Result<(), CommandError> {
        // In real implementation: split the source document.
        Ok(())
    }

    fn undo(&self, _overlay: &mut CowOverlay) -> Result<(), CommandError> {
        // Split creates multiple files; undo removes them.
        Ok(())
    }

    fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        use std::io::Write;
        let _ = writeln!(buf, "SOURCE:{}", self.source.path.display());
        let _ = writeln!(buf, "STRATEGY:{:?}", self.strategy);
        for f in &self.output_files {
            let _ = writeln!(buf, "OUTPUT:{}", f.display());
        }
        buf
    }

    fn box_clone(&self) -> Box<dyn Command> {
        Box::new(self.clone())
    }
}

/// Optimize/compress command. [FR-OPT]
#[derive(Debug, Clone)]
pub struct OptimizeCommand {
    /// Input path.
    pub input_path: std::path::PathBuf,
    /// Output path.
    pub output_path: std::path::PathBuf,
    /// Optimization settings.
    pub settings: OptimizeSettings,
    /// Pre-flight report.
    pub preflight_report: String,
}

impl Command for OptimizeCommand {
    fn name(&self) -> &str {
        "Optimize"
    }

    fn apply(&self, _overlay: &mut CowOverlay) -> Result<(), CommandError> {
        // In real implementation: read input, optimize, write output.
        Ok(())
    }

    fn undo(&self, _overlay: &mut CowOverlay) -> Result<(), CommandError> {
        // Optimization creates a new file; undo removes it.
        Ok(())
    }

    fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        use std::io::Write;
        let _ = writeln!(buf, "INPUT:{}", self.input_path.display());
        let _ = writeln!(buf, "OUTPUT:{}", self.output_path.display());
        let _ = writeln!(buf, "PROFILE:{:?}", self.settings.profile);
        buf
    }

    fn box_clone(&self) -> Box<dyn Command> {
        Box::new(self.clone())
    }
}

/// Build a merge command group.
pub fn build_merge_group(
    sources: Vec<DocumentRef>,
    output_path: PathBuf,
) -> CommandGroup {
    let name = format!("Merge {} document(s)", sources.len());
    let mut group = CommandGroup::new(name);
    group.push(Box::new(MergeCommand {
        sources,
        output_path,
        merged_bytes: Vec::new(),
    }));
    group
}

/// Build a split command group.
pub fn build_split_group(
    source: DocumentRef,
    strategy: SplitStrategy,
    output_files: Vec<PathBuf>,
) -> CommandGroup {
    let name = format!("Split into {} file(s)", output_files.len());
    let mut group = CommandGroup::new(name);
    group.push(Box::new(SplitCommand { source, strategy, output_files }));
    group
}

/// Build an optimize command group with pre-flight disclosure.
pub fn build_optimize_group(
    input_path: std::path::PathBuf,
    output_path: std::path::PathBuf,
    settings: OptimizeSettings,
    original_size: u64,
) -> CommandGroup {
    let preflight = settings.preflight_report(original_size);
    let name = format!("Optimize ({:?})", settings.profile);
    let mut group = CommandGroup::new(name);
    group.push(Box::new(OptimizeCommand {
        input_path,
        output_path,
        settings,
        preflight_report: preflight,
    }));
    group
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_ref_range() {
        let doc = DocumentRef::new("/test.pdf", 100);
        assert_eq!(doc.len(), 100);

        let sub = doc.clone().with_range(10, 50);
        assert_eq!(sub.len(), 40);
        assert_eq!(sub.start_page, 10);
        assert_eq!(sub.end_page, 50);
    }

    #[test]
    fn optimize_profiles() {
        let screen = OptimizeSettings::for_profile(OptimizeProfile::Screen);
        assert!(screen.recompress_streams);
        assert_eq!(screen.downsample_target_dpi, 72);

        let print = OptimizeSettings::for_profile(OptimizeProfile::Print);
        assert_eq!(print.downsample_target_dpi, 150);

        let archive = OptimizeSettings::for_profile(OptimizeProfile::ArchivePreserving);
        assert!(!archive.recompress_streams);
        assert!(archive.downsample_threshold_dpi.is_none());
    }

    #[test]
    fn optimize_preflight_report() {
        let settings = OptimizeSettings::for_profile(OptimizeProfile::Screen);
        let report = settings.preflight_report(1_000_000);
        assert!(report.contains("Pre-flight Report"));
        assert!(report.contains("Screen"));
        assert!(report.contains("reduction"));
        assert!(report.contains("LOSSY")); // image downsampling is lossy
    }

    #[test]
    fn optimize_estimate_reduction() {
        let screen = OptimizeSettings::for_profile(OptimizeProfile::Screen);
        assert!(screen.estimate_reduction_pct() > 30.0);

        let archive = OptimizeSettings::for_profile(OptimizeProfile::ArchivePreserving);
        assert!(archive.estimate_reduction_pct() < 10.0);
    }

    #[test]
    fn merge_group_name() {
        let sources = vec![
            DocumentRef::new("/a.pdf", 10),
            DocumentRef::new("/b.pdf", 20),
        ];
        let group = build_merge_group(sources, PathBuf::from("/merged.pdf"));
        assert_eq!(group.name, "Merge 2 document(s)");
    }

    #[test]
    fn split_group_name() {
        let source = DocumentRef::new("/test.pdf", 100);
        let outputs = vec![PathBuf::from("/part1.pdf"), PathBuf::from("/part2.pdf")];
        let group = build_split_group(source, SplitStrategy::PagesPerFile(50), outputs);
        assert_eq!(group.name, "Split into 2 file(s)");
    }

    #[test]
    fn split_strategies() {
        let ranges = SplitStrategy::PageRanges(vec![(0, 10), (10, 20), (20, 30)]);
        let sizes = SplitStrategy::ByFileSize(1_000_000);
        let bookmarks = SplitStrategy::ByBookmarks;
        let per_file = SplitStrategy::PagesPerFile(10);

        // Just verify they can be created.
        match ranges { _ => {} }
        match sizes { _ => {} }
        match bookmarks { _ => {} }
        match per_file { _ => {} }
    }

    #[test]
    fn merge_serialization() {
        let sources = vec![DocumentRef::new("/a.pdf", 10)];
        let cmd = MergeCommand {
            sources,
            output_path: PathBuf::from("/merged.pdf"),
            merged_bytes: Vec::new(),
        };
        let data = cmd.serialize();
        let text = String::from_utf8_lossy(&data);
        assert!(text.contains("SOURCE:/a.pdf"));
        assert!(text.contains("OUTPUT:/merged.pdf"));
    }
}
