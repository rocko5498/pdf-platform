//! Optimization profiles and their pre-flight disclosure. [FR-OPT-1]
//!
//! Merge, split and extract themselves live in `assembly_ops`, which drives
//! qpdf over whole files. This module used to also carry `MergeCommand`,
//! `SplitCommand` and `OptimizeCommand` — three `Command` implementations
//! whose `apply` was an empty body returning `Ok(())`, with a comment saying a
//! real implementation would do the work. Nothing constructed them outside
//! their own tests, and those tests asserted the *name* of the command group;
//! one asserted only that four enum variants could be constructed. Had
//! anything ever routed assembly through the journal, it would have reported
//! success and produced no file. Deleted rather than left as a facade.
//! [PRIN-6, GR-8, ADR-012]


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

#[cfg(test)]
mod tests {
    use super::*;

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

}
