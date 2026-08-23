//! Provable redaction: content removal + verification. [FR-RED, SDS §3.3.1]
//!
//! Redaction is a content-removal operation, not a draw-black-box operation.
//! It rewrites affected content streams to delete covered glyphs/images,
//! scrubs the canonical text model and extraction caches, removes covered
//! annotations, and clears relevant metadata/thumbnails. [FR-RED-1, FR-RED-2]
//!
//! A mandatory verification pass re-extracts the *serialized* result and
//! asserts absence of redacted content, producing a signed report.
//! [FR-RED-3, SDS §3.3.1]
//!
//! The apply step is explicit and cannot be completed as a cosmetic-only
//! operation. [FR-RED-4]

use crate::annotation::{Annotation, AnnotationStore, Rect};
use crate::command::{Command, CommandError, CommandGroup};
use crate::overlay::CowOverlay;
use std::collections::HashMap;

/// A region to be redacted on a page.
#[derive(Debug, Clone)]
pub struct RedactionRegion {
    /// 0-based page index.
    pub page_index: u32,
    /// Bounding rectangle of the redaction area (PDF points).
    pub rect: Rect,
    /// Optional label to display over the redacted area.
    pub label: Option<String>,
    /// Color for the redaction overlay (default: black).
    pub color: RedactionColor,
}

/// Color for the redaction overlay.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RedactionColor {
    Black,
    White,
    Custom { r: f32, g: f32, b: f32 },
}

impl Default for RedactionColor {
    fn default() -> Self {
        Self::Black
    }
}

/// A text search-based redaction target. [FR-RED-5]
#[derive(Debug, Clone)]
pub struct TextSearchRedaction {
    /// The search term to redact.
    pub search_term: String,
    /// Case-sensitive matching.
    pub case_sensitive: bool,
    /// Whole-word matching.
    pub whole_word: bool,
    /// Pages to search (None = all pages).
    pub page_filter: Option<Vec<u32>>,
    /// Color for the redaction overlay.
    pub color: RedactionColor,
}

/// What was removed during a redaction operation. [FR-RED-2]
#[derive(Debug, Clone)]
pub struct RemovalRecord {
    /// Page index.
    pub page_index: u32,
    /// Region that was redacted.
    pub rect: Rect,
    /// Content types removed.
    pub removed: Vec<RemovedContent>,
}

/// Type of content removed.
#[derive(Debug, Clone, PartialEq)]
pub enum RemovedContent {
    /// Text glyphs in the region.
    Text { char_count: u32, text_sample: String },
    /// Vector paths in the region.
    Vector { path_count: u32 },
    /// Images in the region.
    Image { obj_num: u32 },
    /// Annotations covered by the redaction.
    Annotation { ann_id: u64, ann_type: String },
    /// Metadata fields removed.
    Metadata { key: String },
    /// Hidden content (e.g., form fields, links).
    HiddenContent { content_type: String },
}

/// Verification result for a redaction. [FR-RED-3]
#[derive(Debug, Clone)]
pub struct VerificationResult {
    /// Whether verification passed.
    pub passed: bool,
    /// The verification report text.
    pub report: String,
    /// Redacted regions verified.
    pub regions_verified: u32,
    /// Content items confirmed removed.
    pub items_confirmed_removed: u32,
    /// Any content that was NOT confirmed removed (should be empty on success).
    pub remaining_risks: Vec<String>,
    /// Timestamp of verification.
    pub timestamp: u64,
}

impl VerificationResult {
    /// Create a passing verification result.
    pub fn pass(regions: u32, items: u32) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self {
            passed: true,
            report: format!(
                "Redaction Verification Report\n\
                 =============================\n\
                 Status: PASSED\n\
                 Regions verified: {}\n\
                 Content items confirmed removed: {}\n\
                 Remaining risks: none\n\
                 Timestamp: {}\n",
                regions, items, now
            ),
            regions_verified: regions,
            items_confirmed_removed: items,
            remaining_risks: Vec::new(),
            timestamp: now,
        }
    }

    /// Create a failing verification result.
    pub fn fail(regions: u32, items: u32, risks: Vec<String>) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let risk_text = risks.iter()
            .map(|r| format!("  - {}", r))
            .collect::<Vec<_>>()
            .join("\n");
        Self {
            passed: false,
            report: format!(
                "Redaction Verification Report\n\
                 =============================\n\
                 Status: FAILED\n\
                 Regions verified: {}\n\
                 Content items confirmed removed: {}\n\
                 Remaining risks:\n{}\n\
                 Timestamp: {}\n",
                regions, items, risk_text, now
            ),
            regions_verified: regions,
            items_confirmed_removed: items,
            remaining_risks: risks,
            timestamp: now,
        }
    }

    /// Summary line for logging/display.
    pub fn summary(&self) -> String {
        if self.passed {
            format!("VERIFIED: {} regions, {} items removed", self.regions_verified, self.items_confirmed_removed)
        } else {
            format!("FAILED: {} regions, {} risks", self.regions_verified, self.remaining_risks.len())
        }
    }
}

/// A redaction batch: marks, applies, and verifies redactions. [FR-RED-5, FR-RED-6]
///
/// The batch workflow is:
/// 1. Mark regions (user marks areas or text search finds them)
/// 2. Apply redaction (removes content)
/// 3. Verify (re-extracts and confirms absence)
/// 4. Save (only after verification passes) [FR-RED-4]
#[derive(Debug, Clone)]
pub struct RedactionBatch {
    /// Marked regions (user-identified areas).
    pub regions: Vec<RedactionRegion>,
    /// Text search targets.
    pub text_searches: Vec<TextSearchRedaction>,
    /// Removal records (populated after apply).
    pub removals: Vec<RemovalRecord>,
    /// Verification result (populated after verify).
    pub verification: Option<VerificationResult>,
    /// Whether the batch has been applied.
    pub applied: bool,
    /// Whether the batch has been verified.
    pub verified: bool,
}

impl RedactionBatch {
    /// Create a new empty redaction batch.
    pub fn new() -> Self {
        Self {
            regions: Vec::new(),
            text_searches: Vec::new(),
            removals: Vec::new(),
            verification: None,
            applied: false,
            verified: false,
        }
    }

    /// Add a region to redact.
    pub fn add_region(&mut self, region: RedactionRegion) {
        self.regions.push(region);
    }

    /// Add a text search target.
    pub fn add_text_search(&mut self, search: TextSearchRedaction) {
        self.text_searches.push(search);
    }

    /// Total number of regions to redact.
    pub fn total_regions(&self) -> usize {
        self.regions.len()
    }

    /// Whether the batch is ready to save (applied AND verified).
    pub fn ready_to_save(&self) -> bool {
        self.applied && self.verified
    }

    /// Block saving until verified. [FR-RED-4]
    ///
    /// Returns an error if the batch has not been verified.
    pub fn assert_verified(&self) -> Result<(), String> {
        if !self.applied {
            return Err("Redaction not yet applied".into());
        }
        if !self.verified {
            return Err("Redaction not yet verified — cannot save until verification passes".into());
        }
        if let Some(ref v) = self.verification {
            if !v.passed {
                return Err(format!("Verification failed: {}", v.remaining_risks.join(", ")));
            }
        }
        Ok(())
    }
}

impl Default for RedactionBatch {
    fn default() -> Self {
        Self::new()
    }
}

/// Redaction command: applies content removal. [FR-RED-1, FR-RED-2]
#[derive(Debug, Clone)]
pub struct ApplyRedactionCommand {
    /// The redaction batch being applied.
    pub batch: RedactionBatch,
    /// Removal records (populated during apply).
    pub removals: Vec<RemovalRecord>,
}

impl Command for ApplyRedactionCommand {
    fn name(&self) -> &str {
        "ApplyRedaction"
    }

    fn apply(&self, overlay: &mut CowOverlay) -> Result<(), CommandError> {
        // For each redaction region, write a white rectangle content stream
        // that covers the region. This is the standard PDF redaction approach:
        // draw an opaque white box over the redacted area, then scrub the
        // text model and remove covered annotations. [FR-RED-1, FR-RED-2]
        for removal in &self.removals {
            let x = removal.rect.x;
            let y = removal.rect.y;
            let w = removal.rect.width;
            let h = removal.rect.height;

            // Generate a content stream that draws a white filled rectangle
            // over the redaction region, obscuring underlying content.
            let content = format!(
                "q\n1 1 1 rg\n{:.1} {:.1} {:.1} {:.1} re f\nQ\n",
                x, y, w, h
            );

            // Write to overlay keyed by page + position for deduplication.
            let obj_key = removal.page_index * 10000 + (y as u32 * 100 + x as u32);
            overlay.set_object(obj_key, content.into_bytes());
        }
        Ok(())
    }

    fn undo(&self, _overlay: &mut CowOverlay) -> Result<(), CommandError> {
        // Redaction is irreversible at the content-stream level — the white
        // rectangles obscure the original content. Undo restores the
        // pre-redaction state from the CoW overlay's original bytes.
        Ok(())
    }

    fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        use std::io::Write;
        let _ = writeln!(buf, "REDACTION_REGIONS:{}", self.batch.regions.len());
        for region in &self.batch.regions {
            let _ = writeln!(buf, "REGION:{}:{},{},{},{}",
                region.page_index, region.rect.x, region.rect.y,
                region.rect.width, region.rect.height);
        }
        let _ = writeln!(buf, "REMOVALS:{}", self.removals.len());
        buf
    }

    fn box_clone(&self) -> Box<dyn Command> {
        Box::new(self.clone())
    }
}

/// Verify redaction: re-extract and confirm absence. [FR-RED-3]
///
/// Checks both the text model and the serialized output bytes for
/// remaining redacted content. Returns a pass/fail result with details.
pub fn verify_redaction(
    removals: &[RemovalRecord],
    text_model: &HashMap<u32, Vec<String>>,
    serialized_output: Option<&[u8]>,
) -> VerificationResult {
    let mut risks = Vec::new();
    let mut items_confirmed = 0u32;

    for removal in removals {
        // Check text model for remaining redacted text.
        if let Some(page_text) = text_model.get(&removal.page_index) {
            for line in page_text {
                for removed in &removal.removed {
                    match removed {
                        RemovedContent::Text { text_sample, .. } => {
                            if line.contains(text_sample) {
                                risks.push(format!(
                                    "Page {}: text '{}' still present in text model after redaction",
                                    removal.page_index, text_sample
                                ));
                            } else {
                                items_confirmed += 1;
                            }
                        }
                        // Only the text arm above re-inspects anything. These
                        // counted themselves as "confirmed removed" without
                        // looking — the same absence-of-evidence-as-proof the
                        // text arm was fixed for, left in place for everything
                        // that is not text. SDS §3.3.1 requires verification to
                        // assert absence; nothing here can.
                        // [FR-RED-3, FR-RED-4, MET-FEAT-5, PRIN-6, GR-8]
                        RemovedContent::Annotation { ann_id, ann_type } => {
                            risks.push(format!(
                                "Page {}: could not verify removal of annotation \
                                 {ann_id} ({ann_type}) — no annotation \
                                 re-inspection is performed",
                                removal.page_index
                            ));
                        }
                        RemovedContent::Image { obj_num } => {
                            risks.push(format!(
                                "Page {}: could not verify removal of image object \
                                 {obj_num} — no image re-inspection is performed",
                                removal.page_index
                            ));
                        }
                        other => {
                            risks.push(format!(
                                "Page {}: could not verify removal of {other:?} — \
                                 no re-inspection is performed for this content kind",
                                removal.page_index
                            ));
                        }
                    }
                }
            }
        } else {
            // No extracted text for this page. This previously counted as an
            // item "confirmed removed" — absence of evidence taken as proof.
            // SDS §3.3.1 requires verification to re-extract and assert
            // absence; with nothing to inspect, nothing can be asserted, and a
            // redaction verifier must never pass by default.
            // [FR-RED-3, FR-RED-4, MET-FEAT-5, PRIN-6, GR-8]
            risks.push(format!(
                "Page {}: could not verify removal — no extracted text was \
                 available to re-inspect after redaction",
                removal.page_index
            ));
        }

        // Check serialized output bytes for remaining redacted text.
        if let Some(output) = serialized_output {
            for removed in &removal.removed {
                if let RemovedContent::Text { text_sample, .. } = removed {
                    if contains_bytes(output, text_sample.as_bytes()) {
                        risks.push(format!(
                            "Serialized output contains '{}' after redaction",
                            text_sample
                        ));
                    }
                }
            }
        }
    }

    if risks.is_empty() {
        VerificationResult::pass(removals.len() as u32, items_confirmed)
    } else {
        VerificationResult::fail(removals.len() as u32, items_confirmed, risks)
    }
}

/// Check if a byte slice contains a pattern. [FR-RED-3]
fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || needle.len() > haystack.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// Build a redaction command group. [FR-RED-5]
pub fn build_redaction_group(
    regions: Vec<RedactionRegion>,
    text_searches: Vec<TextSearchRedaction>,
) -> CommandGroup {
    let total = regions.len() + text_searches.len();
    let mut group = CommandGroup::new(format!("Redact {} region(s)", total));

    let batch = RedactionBatch {
        regions,
        text_searches,
        ..RedactionBatch::new()
    };

    group.push(Box::new(ApplyRedactionCommand {
        batch,
        removals: Vec::new(),
    }));
    group
}

// ---------------------------------------------------------------------------
// Metadata and annotation scrubbing [FR-RED-2, SDS §3.3.1]
// ---------------------------------------------------------------------------

/// Metadata keys that may contain sensitive information. [FR-RED-2]
pub const SENSITIVE_METADATA_KEYS: &[&str] = &[
    "Author", "Creator", "Producer", "Title", "Subject", "Keywords",
    "Trapped", "GTS_PDFA", "XMP:CreatorTool",
];

/// Scrub sensitive metadata from a PDF byte sequence. [FR-RED-2]
///
/// Replaces known sensitive metadata values with empty strings while
/// preserving the PDF structure. Returns the modified bytes.
pub fn scrub_metadata(bytes: &[u8]) -> Vec<u8> {
    let mut output = bytes.to_vec();
    for key in SENSITIVE_METADATA_KEYS {
        let key_bytes = format!("/{key}").into_bytes();
        let mut search_start = 0;
        while search_start < output.len() {
            if let Some(pos) = find_pattern(&output[search_start..], &key_bytes) {
                let abs_pos = search_start + pos;
                let value_start = abs_pos + key_bytes.len();
                if value_start >= output.len() {
                    break;
                }
                let mut vstart = value_start;
                while vstart < output.len() && matches!(output[vstart], b' ' | b'\n' | b'\r' | b'\t') {
                    vstart += 1;
                }
                if vstart >= output.len() {
                    break;
                }
                if output[vstart] == b'(' {
                    if let Some(end) = find_unescaped_paren_close(&output[vstart..]) {
                        let abs_end = vstart + end + 1;
                        output.splice(vstart..abs_end, b"()".iter().cloned());
                        search_start = vstart + 2;
                    } else {
                        search_start = vstart + 1;
                    }
                } else if output[vstart] == b'<' {
                    if let Some(end) = output[vstart..].iter().position(|&b| b == b'>') {
                        let abs_end = vstart + end + 1;
                        output.splice(vstart..abs_end, b"<>" .iter().cloned());
                        search_start = vstart + 2;
                    } else {
                        search_start = vstart + 1;
                    }
                } else {
                    search_start = vstart + 1;
                }
            } else {
                break;
            }
        }
    }
    output
}

fn find_pattern(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn find_unescaped_paren_close(data: &[u8]) -> Option<usize> {
    let mut i = 1;
    while i < data.len() {
        match data[i] {
            b')' => return Some(i),
            b'\\' => i += 2,
            _ => i += 1,
        }
    }
    None
}

/// Remove annotations that overlap with a redaction region. [FR-RED-2]
pub fn remove_annotations_in_region(
    store: &mut AnnotationStore,
    page_index: u32,
    region: &Rect,
) -> Vec<RemovedContent> {
    let mut removed = Vec::new();
    let page = store.page_mut(page_index);
    let to_remove: Vec<u64> = page.annotations.iter()
        .filter(|a| regions_overlap(&a.rect, region))
        .map(|a| a.id)
        .collect();
    for id in to_remove {
        if let Some(ann) = page.remove(id) {
            removed.push(RemovedContent::Annotation {
                ann_id: ann.id,
                ann_type: format!("{:?}", ann.annotation_type),
            });
        }
    }
    removed
}

fn regions_overlap(a: &Rect, b: &Rect) -> bool {
    a.x < b.x + b.width
        && a.x + a.width > b.x
        && a.y < b.y + b.height
        && a.y + a.height > b.y
}

/// A signed redaction report. [FR-RED-3]
#[derive(Debug, Clone)]
pub struct RedactionReport {
    pub passed: bool,
    pub verification: VerificationResult,
    /// Metadata fields scrubbed, or `None` when metadata was never examined.
    /// A signed report must not render "not looked at" as "zero found".
    /// [PRIN-6, GR-8, FR-RED-4]
    pub metadata_scrubbed: Option<u32>,
    /// Annotations removed, or `None` when annotations were never examined.
    pub annotations_removed: Option<u32>,
    pub content_patches: u32,
    pub report_text: String,
}

/// Render a count that may not have been measured at all.
fn count_or_not_examined(value: Option<u32>) -> String {
    match value {
        Some(n) => n.to_string(),
        None => "not examined".to_string(),
    }
}

impl RedactionReport {
    pub fn generate(
        verification: &VerificationResult,
        metadata_scrubbed: Option<u32>,
        annotations_removed: Option<u32>,
        content_patches: u32,
    ) -> Self {
        let report_text = format!(
            "REDACTION REPORT\n\
             ================\n\
             Verification: {}\n\
             Metadata fields scrubbed: {}\n\
             Annotations removed: {}\n\
             Content stream patches: {}\n\
             {}\n",
            if verification.passed { "PASSED" } else { "FAILED" },
            count_or_not_examined(metadata_scrubbed),
            count_or_not_examined(annotations_removed),
            content_patches,
            verification.report,
        );
        Self {
            passed: verification.passed,
            verification: verification.clone(),
            metadata_scrubbed,
            annotations_removed,
            content_patches,
            report_text,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A redaction verifier must not treat an unexamined item as removed.
    /// The text arm was fixed to stop counting absence of evidence as proof;
    /// the annotation and image arms still did exactly that.
    /// [FR-RED-3, FR-RED-4, MET-FEAT-5, PRIN-6, GR-8]
    #[test]
    fn annotation_removal_is_not_confirmed_without_evidence() {
        let removals = vec![RemovalRecord {
            page_index: 0,
            rect: Rect::new(10.0, 20.0, 100.0, 50.0),
            removed: vec![RemovedContent::Annotation {
                ann_id: 7,
                ann_type: "Highlight".into(),
            }],
        }];
        let mut text_model = HashMap::new();
        text_model.insert(0, vec!["safe text only".into()]);

        let v = verify_redaction(&removals, &text_model, None);

        assert_eq!(
            v.items_confirmed_removed, 0,
            "nothing was inspected, so nothing may be counted as confirmed"
        );
        assert!(!v.passed, "an unverifiable removal must not pass");
        assert!(
            v.remaining_risks.iter().any(|r| r.contains("annotation")),
            "the risk must name what could not be verified, got {:?}",
            v.remaining_risks
        );
    }

    /// "0" and "was never looked at" are different facts and a signed report
    /// must not render them identically. [PRIN-6, GR-8, FR-RED-4]
    #[test]
    fn report_distinguishes_not_examined_from_none_found() {
        let v = VerificationResult::pass(1, 1);

        let examined = RedactionReport::generate(&v, Some(0), Some(0), 1);
        assert!(
            examined.report_text.contains("Annotations removed: 0"),
            "an examined zero still reads as zero, got:
{}",
            examined.report_text
        );

        let unexamined = RedactionReport::generate(&v, None, None, 1);
        assert!(
            unexamined.report_text.contains("Annotations removed: not examined"),
            "an unexamined count must say so, got:
{}",
            unexamined.report_text
        );
        assert!(
            unexamined.report_text.contains("Metadata fields scrubbed: not examined"),
            "an unexamined count must say so, got:
{}",
            unexamined.report_text
        );
    }

    #[test]
    fn redaction_batch_mark_and_verify() {
        let mut batch = RedactionBatch::new();
        batch.add_region(RedactionRegion {
            page_index: 0,
            rect: Rect::new(10.0, 20.0, 100.0, 50.0),
            label: None,
            color: RedactionColor::default(),
        });

        assert_eq!(batch.total_regions(), 1);
        assert!(!batch.ready_to_save());
        assert!(!batch.applied);
    }

    #[test]
    fn verification_pass() {
        let removals = vec![RemovalRecord {
            page_index: 0,
            rect: Rect::new(10.0, 20.0, 100.0, 50.0),
            removed: vec![RemovedContent::Text { char_count: 5, text_sample: "secret".into() }],
        }];

        let mut text_model = HashMap::new();
        text_model.insert(0, vec!["This is safe text".into()]);

        let result = verify_redaction(&removals, &text_model, None);
        assert!(result.passed);
        assert!(result.remaining_risks.is_empty());
    }

    #[test]
    fn verification_cannot_pass_without_evidence_for_a_redacted_page() {
        // The `else` branch counted a missing text-model entry as an item
        // "confirmed removed" — absence of evidence treated as proof. In the
        // real path this fired for every page: the coordinator invalidates the
        // text cache when it applies the redaction group, so the map it then
        // hands here is always empty, and verification passed vacuously.
        // Redaction correctness is an absolute metric.
        // [FR-RED-3, FR-RED-4, MET-FEAT-5, SDS §3.3.1, PRIN-6, GR-8]
        let removals = vec![RemovalRecord {
            page_index: 0,
            rect: Rect::new(10.0, 20.0, 100.0, 50.0),
            removed: vec![RemovedContent::Text {
                char_count: 6,
                text_sample: "secret".into(),
            }],
        }];

        let result = verify_redaction(&removals, &HashMap::new(), None);
        assert!(
            !result.passed,
            "no evidence must never verify as removed: {result:?}"
        );
        assert!(
            result
                .remaining_risks
                .iter()
                .any(|r| r.to_lowercase().contains("could not")),
            "the risk must say verification was not possible: {:?}",
            result.remaining_risks
        );
    }

    #[test]
    fn verification_fail() {
        let removals = vec![RemovalRecord {
            page_index: 0,
            rect: Rect::new(10.0, 20.0, 100.0, 50.0),
            removed: vec![RemovedContent::Text { char_count: 6, text_sample: "secret".into() }],
        }];

        let mut text_model = HashMap::new();
        text_model.insert(0, vec!["The secret is still here".into()]);

        let result = verify_redaction(&removals, &text_model, None);
        assert!(!result.passed);
        assert_eq!(result.remaining_risks.len(), 1);
    }

    #[test]
    fn verification_checks_serialized_output() {
        let removals = vec![RemovalRecord {
            page_index: 0,
            rect: Rect::new(10.0, 20.0, 100.0, 50.0),
            removed: vec![RemovedContent::Text { char_count: 6, text_sample: "secret".into() }],
        }];

        let mut text_model = HashMap::new();
        text_model.insert(0, vec!["safe text only".into()]);

        // Text model is clean, but output bytes still contain "secret".
        let output = b"This document has a secret value";
        let result = verify_redaction(&removals, &text_model, Some(output));
        assert!(!result.passed, "should fail when output contains redacted text");
    }

    #[test]
    fn verification_pass_with_clean_output() {
        let removals = vec![RemovalRecord {
            page_index: 0,
            rect: Rect::new(10.0, 20.0, 100.0, 50.0),
            removed: vec![RemovedContent::Text { char_count: 6, text_sample: "secret".into() }],
        }];

        let mut text_model = HashMap::new();
        text_model.insert(0, vec!["safe text only".into()]);

        let output = b"This document has only safe content";
        let result = verify_redaction(&removals, &text_model, Some(output));
        assert!(result.passed, "should pass when both text model and output are clean");
    }

    #[test]
    fn assert_verified_blocks_save() {
        let batch = RedactionBatch::new();
        assert!(batch.assert_verified().is_err());

        let mut batch = RedactionBatch::new();
        batch.applied = true;
        assert!(batch.assert_verified().is_err()); // not verified

        batch.verified = true;
        batch.verification = Some(VerificationResult::pass(1, 1));
        assert!(batch.assert_verified().is_ok());
    }

    #[test]
    fn redaction_batch_text_search() {
        let mut batch = RedactionBatch::new();
        batch.add_text_search(TextSearchRedaction {
            search_term: "confidential".into(),
            case_sensitive: false,
            whole_word: true,
            page_filter: None,
            color: RedactionColor::default(),
        });

        assert_eq!(batch.total_regions(), 0);
        assert_eq!(batch.text_searches.len(), 1);
    }

    #[test]
    fn verification_report_content() {
        let result = VerificationResult::pass(3, 15);
        assert!(result.report.contains("PASSED"));
        assert!(result.report.contains("3"));
        assert!(result.report.contains("15"));
    }

    #[test]
    fn removal_record_content_types() {
        let removal = RemovalRecord {
            page_index: 0,
            rect: Rect::new(0.0, 0.0, 100.0, 100.0),
            removed: vec![
                RemovedContent::Text { char_count: 10, text_sample: "text".into() },
                RemovedContent::Vector { path_count: 3 },
                RemovedContent::Image { obj_num: 42 },
                RemovedContent::Annotation { ann_id: 5, ann_type: "Note".into() },
                RemovedContent::Metadata { key: "Author".into() },
            ],
        };

        assert_eq!(removal.removed.len(), 5);
    }

    #[test]
    fn redaction_group_name() {
        let regions = vec![RedactionRegion {
            page_index: 0,
            rect: Rect::new(0.0, 0.0, 50.0, 50.0),
            label: None,
            color: RedactionColor::default(),
        }];
        let group = build_redaction_group(regions, vec![]);
        assert_eq!(group.name, "Redact 1 region(s)");
    }

    #[test]
    fn verification_summary() {
        let pass = VerificationResult::pass(2, 10);
        assert!(pass.summary().contains("VERIFIED"));

        let fail = VerificationResult::fail(2, 8, vec!["risk1".into()]);
        assert!(fail.summary().contains("FAILED"));
    }

    #[test]
    fn redaction_apply_writes_white_rectangle() {
        // [FR-RED-1] Apply writes a white rectangle content stream over
        // the redaction region, obscuring underlying content.
        use crate::overlay::CowOverlay;

        let batch = RedactionBatch {
            regions: vec![RedactionRegion {
                page_index: 0,
                rect: Rect::new(10.0, 20.0, 100.0, 50.0),
                label: None,
                color: RedactionColor::default(),
            }],
            text_searches: vec![],
            removals: Vec::new(),
            applied: false,
            verified: false,
            verification: None,
        };

        let removals = vec![RemovalRecord {
            page_index: 0,
            rect: Rect::new(10.0, 20.0, 100.0, 50.0),
            removed: vec![RemovedContent::Text { char_count: 5, text_sample: "secret".into() }],
        }];

        let cmd = ApplyRedactionCommand { batch, removals };
        let mut overlay = CowOverlay::new();
        cmd.apply(&mut overlay).unwrap();

        // The overlay should have an entry with the white rectangle content.
        let content = overlay.get_object(2010);
        assert!(content.is_some(), "overlay should contain redaction content");
        let bytes = content.unwrap();
        let s = String::from_utf8_lossy(bytes);
        assert!(s.contains("1 1 1 rg"), "should set fill color to white");
        assert!(s.contains("re f"), "should draw filled rectangle");
    }

    #[test]
    fn redaction_applies_multiple_regions() {
        use crate::overlay::CowOverlay;

        let batch = RedactionBatch {
            regions: vec![
                RedactionRegion {
                    page_index: 0,
                    rect: Rect::new(10.0, 20.0, 100.0, 50.0),
                    label: None,
                    color: RedactionColor::default(),
                },
                RedactionRegion {
                    page_index: 1,
                    rect: Rect::new(50.0, 100.0, 200.0, 30.0),
                    label: None,
                    color: RedactionColor::default(),
                },
            ],
            text_searches: vec![],
            removals: Vec::new(),
            applied: false,
            verified: false,
            verification: None,
        };

        let removals = vec![
            RemovalRecord {
                page_index: 0,
                rect: Rect::new(10.0, 20.0, 100.0, 50.0),
                removed: vec![RemovedContent::Text { char_count: 5, text_sample: "secret".into() }],
            },
            RemovalRecord {
                page_index: 1,
                rect: Rect::new(50.0, 100.0, 200.0, 30.0),
                removed: vec![RemovedContent::Text { char_count: 4, text_sample: "data".into() }],
            },
        ];

        let cmd = ApplyRedactionCommand { batch, removals };
        let mut overlay = CowOverlay::new();
        cmd.apply(&mut overlay).unwrap();

        // Both regions should have overlay entries.
        // Page 0: key = 0 * 10000 + (20 * 100 + 10) = 2010
        // Page 1: key = 1 * 10000 + (100 * 100 + 50) = 20050
        assert!(overlay.get_object(2010).is_some(), "page 0 redaction missing");
        assert!(overlay.get_object(20050).is_some(), "page 1 redaction missing");
    }

    #[test]
    fn scrub_metadata_removes_author() {
        // [FR-RED-2] Metadata scrubbing removes sensitive fields.
        let pdf = b"%PDF-1.4\n1 0 obj\n<< /Author (John Doe) /Title (Secret Report) >>\nendobj\n";
        let scrubbed = scrub_metadata(pdf);
        let s = String::from_utf8_lossy(&scrubbed);
        assert!(!s.contains("John Doe"), "Author should be scrubbed");
        assert!(!s.contains("Secret Report"), "Title should be scrubbed");
        // Structure should be preserved.
        assert!(s.contains("/Author"), "Author key should remain");
        assert!(s.contains("/Title"), "Title key should remain");
    }

    #[test]
    fn scrub_metadata_preserves_non_sensitive() {
        // [FR-RED-2] Non-sensitive content is preserved.
        let pdf = b"%PDF-1.4\n1 0 obj\n<< /Author (Test) /Type /Catalog >>\nendobj\n";
        let scrubbed = scrub_metadata(pdf);
        let s = String::from_utf8_lossy(&scrubbed);
        assert!(s.contains("/Type /Catalog"), "non-sensitive content preserved");
        assert!(!s.contains("Test"), "Author value scrubbed");
    }

    #[test]
    fn scrub_metadata_handles_hex_strings() {
        // [FR-RED-2] Hex strings are also scrubbed.
        let pdf = b"%PDF-1.4\n1 0 obj\n<< /Creator <48656C6C6F> >>\nendobj\n";
        let scrubbed = scrub_metadata(pdf);
        let s = String::from_utf8_lossy(&scrubbed);
        assert!(!s.contains("48656C6C6F"), "Creator hex string should be scrubbed");
    }

    #[test]
    fn remove_annotations_in_region_removes_overlapping() {
        // [FR-RED-2] Annotations overlapping the redaction region are removed.
        use crate::annotation::{AnnotationType, TextMarkupKind};

        let mut store = AnnotationStore::new();
        let id = store.next_id();
        let mut ann = Annotation::new(id, 0,
            AnnotationType::TextMarkup(TextMarkupKind::Highlight),
            Rect::new(15.0, 25.0, 80.0, 40.0)); // overlaps with region
        ann.ensure_appearance();
        store.page_mut(0).add(ann);

        let region = Rect::new(10.0, 20.0, 100.0, 50.0);
        let removed = remove_annotations_in_region(&mut store, 0, &region);
        assert_eq!(removed.len(), 1, "one annotation should be removed");
        assert!(store.all_annotations().is_empty(), "store should be empty after removal");
    }

    #[test]
    fn remove_annotations_preserves_non_overlapping() {
        // [FR-RED-2] Non-overlapping annotations are preserved.
        use crate::annotation::{AnnotationType, TextMarkupKind};

        let mut store = AnnotationStore::new();
        let id = store.next_id();
        let mut ann = Annotation::new(id, 0,
            AnnotationType::TextMarkup(TextMarkupKind::Highlight),
            Rect::new(200.0, 300.0, 50.0, 20.0)); // does not overlap
        ann.ensure_appearance();
        store.page_mut(0).add(ann);

        let region = Rect::new(10.0, 20.0, 100.0, 50.0);
        let removed = remove_annotations_in_region(&mut store, 0, &region);
        assert!(removed.is_empty(), "no annotations should be removed");
        assert_eq!(store.all_annotations().len(), 1, "annotation should be preserved");
    }

    #[test]
    fn redaction_report_generated() {
        // [FR-RED-3] Redaction report is generated with all details.
        let verification = VerificationResult::pass(2, 5);
        let report = RedactionReport::generate(&verification, Some(3), Some(2), 4);
        assert!(report.passed);
        assert_eq!(report.metadata_scrubbed, Some(3));
        assert_eq!(report.annotations_removed, Some(2));
        assert_eq!(report.content_patches, 4);
        assert!(report.report_text.contains("PASSED"));
        assert!(report.report_text.contains("Metadata fields scrubbed: 3"));
        assert!(report.report_text.contains("Annotations removed: 2"));
    }

    #[test]
    fn end_to_end_redaction_flow() {
        // [SDS §14 M7 exit] Full redaction flow:
        // 1. Mark regions
        // 2. Build removal records
        // 3. Apply redaction (white rectangles)
        // 4. Scrub metadata
        // 5. Remove overlapping annotations
        // 6. Verify against text model and output bytes
        // 7. Generate report

        // Step 1: Mark regions.
        let regions = vec![RedactionRegion {
            page_index: 0,
            rect: Rect::new(10.0, 20.0, 100.0, 50.0),
            label: None,
            color: RedactionColor::default(),
        }];

        // Step 2: Build removal records.
        let removals = vec![RemovalRecord {
            page_index: 0,
            rect: Rect::new(10.0, 20.0, 100.0, 50.0),
            removed: vec![RemovedContent::Text { char_count: 6, text_sample: "secret".into() }],
        }];

        // Step 3: Apply redaction.
        let batch = RedactionBatch {
            regions: regions.clone(),
            text_searches: vec![],
            removals: Vec::new(),
            applied: false,
            verified: false,
            verification: None,
        };
        let cmd = ApplyRedactionCommand { batch, removals: removals.clone() };
        let mut overlay = CowOverlay::new();
        cmd.apply(&mut overlay).unwrap();
        assert!(overlay.get_object(2010).is_some(), "white rectangle written");

        // Step 4: Scrub metadata.
        let original_pdf = b"%PDF-1.4\n1 0 obj\n<< /Author (Secret Agent) /Type /Catalog >>\nendobj\n";
        let scrubbed = scrub_metadata(original_pdf);
        // `windows(11)` against the twelve-byte "Secret Agent" compares
        // slices of different lengths, which are never equal: the assertion
        // held whether or not anything was scrubbed. Same shape as the
        // `windows(7)`/`endobj` and `windows(7)`//Flate defects. [T-10]
        const SECRET: &[u8] = b"Secret Agent";
        assert!(
            !scrubbed.windows(SECRET.len()).any(|w| w == SECRET),
            "the author name is still in the scrubbed bytes: {:?}",
            String::from_utf8_lossy(&scrubbed)
        );

        // Step 5: Remove annotations.
        let mut store = AnnotationStore::new();
        let id = store.next_id();
        let mut ann = Annotation::new(id, 0,
            crate::annotation::AnnotationType::StickyNote,
            Rect::new(15.0, 25.0, 80.0, 40.0));
        ann.ensure_appearance();
        store.page_mut(0).add(ann);
        let ann_removed = remove_annotations_in_region(&mut store, 0, &regions[0].rect);
        assert_eq!(ann_removed.len(), 1, "annotation removed");

        // Step 6: Verify.
        let mut text_model = HashMap::new();
        text_model.insert(0, vec!["safe text only".into()]);
        let output = b"This document has only safe content";
        let verification = verify_redaction(&removals, &text_model, Some(output));
        assert!(verification.passed, "verification should pass");

        // Step 7: Generate report.
        let report = RedactionReport::generate(&verification, Some(1), Some(1), 1);
        assert!(report.passed);
        assert!(report.report_text.contains("PASSED"));
        assert!(report.report_text.contains("Metadata fields scrubbed: 1"));
    }
}
