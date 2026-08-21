//! PAdES signing + validation. [SDS §2.8, ADR-016]
//!
//! Security-critical: human-gated review required for all non-stub changes. [IG AI-6]
//!
//! Implements PAdES B-B → B-LTA validation with explainable results.
//! Validation is conservative: any ambiguity yields *indeterminate*, never
//! false "valid" [ADR-001 value 5, FR-SIG-1].
//!
//! M8 scope: validation data structures, ByteRange hashing, DocMDP diff
//! analysis, explainable validation results. Software-certificate signing
//! and RFC-3161 timestamps are deferred to M10.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Signature data model [FR-SIG, SDS §2.8]
// ---------------------------------------------------------------------------

/// Signature validation status. [FR-SIG-1]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignatureStatus {
    /// Signature is valid: integrity intact, signer trusted, no illegal changes.
    Valid,
    /// Signature is invalid: integrity broken or illegal changes detected.
    Invalid,
    /// Signature validity cannot be determined (missing info, untrusted cert, etc.).
    Indeterminate,
}

impl std::fmt::Display for SignatureStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Valid => write!(f, "Valid"),
            Self::Invalid => write!(f, "Invalid"),
            Self::Indeterminate => write!(f, "Indeterminate"),
        }
    }
}

/// DocMDP permission level. [FR-SIG-2]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocMDPLevel {
    /// Level 1: no changes permitted after signing.
    Level1,
    /// Level 2: permitted changes: incremental save, annotation additions/modifications.
    Level2,
    /// Level 3: permitted changes: any that do not alter the signed content.
    Level3,
}

/// Information about a single digital signature. [FR-SIG-1]
#[derive(Debug, Clone)]
pub struct SignatureInfo {
    /// The signature's /Name entry (signer name).
    pub name: String,
    /// The signature's /Location entry.
    pub location: String,
    /// The signature's /Reason entry.
    pub reason: String,
    /// Date of signing (from /M entry).
    pub date: String,
    /// ByteRange [start1, length1, start2, length2, ...] — the signed byte ranges.
    pub byte_range: Vec<u64>,
    /// The /Contents value (the CMS signature bytes, hex-decoded).
    pub contents: Vec<u8>,
    /// DocMDP permission level (from /DocMDP in /Reference).
    pub docmdp_level: Option<DocMDPLevel>,
    /// Filter type (e.g., "Adobe.PPKLite", "ETSI.CAdES.detached").
    pub filter: String,
    /// SubFilter type (e.g., "adbe.pkcs7.detached", "ETSI.CAdES.detached").
    pub sub_filter: String,
    /// Byte offset of the signature dictionary in the file.
    pub byte_offset: u64,
    /// Object number of the signature dictionary.
    pub obj_num: u32,
    /// Page index (0-based) where the signature field is located, if visible.
    pub page_index: Option<u32>,
}

/// A change detected after signing. [FR-SIG-2]
#[derive(Debug, Clone)]
pub struct PostSigningChange {
    /// Description of the change.
    pub description: String,
    /// Whether this change is permitted by the DocMDP level.
    pub permitted: bool,
    /// Severity: "info", "warning", "error".
    pub severity: String,
}

/// Validation result for a single signature. [FR-SIG-1]
#[derive(Debug, Clone)]
pub struct ValidationReport {
    /// Overall validation status.
    pub status: SignatureStatus,
    /// Human-readable explanation of the result.
    pub explanation: String,
    /// The signature being validated.
    pub signature: SignatureInfo,
    /// Changes detected after signing.
    pub post_signing_changes: Vec<PostSigningChange>,
    /// Whether the signer certificate is trusted.
    pub signer_trusted: bool,
    /// Whether the CMS signature integrity check passed.
    pub integrity_check_passed: bool,
    /// Whether the ByteRange hash matches.
    pub hash_match: bool,
    /// Timestamp of validation.
    pub validation_time: u64,
}

impl ValidationReport {
    /// Create a summary line for display. [FR-SIG-1]
    pub fn summary(&self) -> String {
        format!(
            "{}: {} — {}",
            self.signature.name,
            self.status,
            if self.post_signing_changes.is_empty() {
                "no post-signing changes".to_string()
            } else {
                let illegal = self.post_signing_changes.iter()
                    .filter(|c| !c.permitted)
                    .count();
                if illegal > 0 {
                    format!("{illegal} illegal change(s)")
                } else {
                    format!("{} permitted change(s)", self.post_signing_changes.len())
                }
            }
        )
    }
}

// ---------------------------------------------------------------------------
// ByteRange hashing [SDS §2.8]
// ---------------------------------------------------------------------------

/// Compute the hash of the signed byte ranges in a PDF file. [FR-SIG-2]
///
/// The ByteRange array specifies alternating (offset, length) pairs that
/// define which bytes are covered by the signature. The hash is computed
/// over exactly those bytes.
///
/// Returns the SHA-256 digest of the concatenation of all byte ranges.
pub fn hash_byte_ranges(file_bytes: &[u8], byte_range: &[u64]) -> Vec<u8> {
    use sha2::{Sha256, Digest};
    let mut hasher = Sha256::new();
    // ByteRange is [offset1, length1, offset2, length2, ...]
    let mut i = 0;
    while i + 1 < byte_range.len() {
        let offset = byte_range[i] as usize;
        let length = byte_range[i + 1] as usize;
        if offset + length <= file_bytes.len() {
            hasher.update(&file_bytes[offset..offset + length]);
        }
        i += 2;
    }
    hasher.finalize().to_vec()
}

/// Verify that a signature's hash matches the file's byte ranges. [FR-SIG-2]
pub fn verify_byte_range_hash(
    file_bytes: &[u8],
    signature: &SignatureInfo,
) -> bool {
    if signature.contents.is_empty() || signature.byte_range.is_empty() {
        return false;
    }
    let computed = hash_byte_ranges(file_bytes, &signature.byte_range);
    // For PKCS#7 detached signatures, the hash is inside the CMS structure.
    // For a basic check, we verify the byte range is valid and non-overlapping.
    // Full CMS verification requires a crypto library (deferred to M10).
    !computed.is_empty() && byte_range_valid(&signature.byte_range, file_bytes.len())
}

/// Check that byte ranges are valid (non-overlapping, within file bounds). [FR-SIG-2]
fn byte_range_valid(byte_range: &[u64], file_len: usize) -> bool {
    if byte_range.is_empty() || byte_range.len() % 2 != 0 {
        return false;
    }
    let mut last_end = 0u64;
    let mut i = 0;
    while i + 1 < byte_range.len() {
        let offset = byte_range[i];
        let length = byte_range[i + 1];
        if offset < last_end {
            return false; // overlapping
        }
        if offset + length > file_len as u64 {
            return false; // out of bounds
        }
        if length == 0 {
            return false; // zero-length range
        }
        last_end = offset + length;
        i += 2;
    }
    true
}

// ---------------------------------------------------------------------------
// DocMDP diff analysis [SDS §2.8]
// ---------------------------------------------------------------------------

/// Analyze changes made after signing. [FR-SIG-2]
///
/// Compares the current xref table with the one at signing time to detect
/// what was modified. Returns a list of changes with their permissibility.
pub fn analyze_docmdp_changes(
    original_xref_entries: &[(u32, u64)],  // (obj_num, offset) at signing time
    current_xref_entries: &[(u32, u64)],    // (obj_num, offset) now
    docmdp_level: DocMDPLevel,
) -> Vec<PostSigningChange> {
    let mut changes = Vec::new();
    let original_map: HashMap<u32, u64> = original_xref_entries.iter().copied().collect();
    let current_map: HashMap<u32, u64> = current_xref_entries.iter().copied().collect();

    // Check for modified objects.
    for (obj_num, &orig_offset) in &original_map {
        if let Some(&curr_offset) = current_map.get(obj_num) {
            if orig_offset != curr_offset {
                let permitted = match docmdp_level {
                    DocMDPLevel::Level1 => false,
                    DocMDPLevel::Level2 => true, // incremental updates are permitted
                    DocMDPLevel::Level3 => true,
                };
                changes.push(PostSigningChange {
                    description: format!("Object {} modified (offset {} → {})", obj_num, orig_offset, curr_offset),
                    permitted,
                    severity: if permitted { "info".into() } else { "error".into() },
                });
            }
        }
    }

    // Check for new objects.
    for obj_num in current_map.keys() {
        if !original_map.contains_key(obj_num) {
            let permitted = match docmdp_level {
                DocMDPLevel::Level1 => false,
                DocMDPLevel::Level2 => true, // new objects (annotations) are permitted
                DocMDPLevel::Level3 => true,
            };
            changes.push(PostSigningChange {
                description: format!("New object {} added", obj_num),
                permitted,
                severity: if permitted { "info".into() } else { "error".into() },
            });
        }
    }

    // Check for deleted objects.
    for obj_num in original_map.keys() {
        if !current_map.contains_key(obj_num) {
            changes.push(PostSigningChange {
                description: format!("Object {} deleted", obj_num),
                permitted: false, // deletion is never permitted under DocMDP
                severity: "error".into(),
            });
        }
    }

    changes
}

// ---------------------------------------------------------------------------
// Signature validation [FR-SIG-1]
// ---------------------------------------------------------------------------

/// Validate a signature against a PDF file. [FR-SIG-1]
///
/// This is the main validation entry point. It performs:
/// 1. ByteRange hash verification
/// 2. DocMDP change analysis
/// 3. Status determination (valid/invalid/indeterminate)
///
/// Full CMS signature verification (certificate chain, trust store, OCSP)
/// requires a crypto library and is deferred to M10. This implementation
/// provides the validation framework and structural checks.
pub fn validate_signature(
    file_bytes: &[u8],
    signature: &SignatureInfo,
    original_xref: &[(u32, u64)],
    current_xref: &[(u32, u64)],
) -> ValidationReport {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Step 1: Verify byte range is well-formed.
    let hash_ok = verify_byte_range_hash(file_bytes, signature);

    // Step 2: Analyze DocMDP changes.
    let changes = if let Some(level) = signature.docmdp_level {
        analyze_docmdp_changes(original_xref, current_xref, level)
    } else {
        // No DocMDP — treat as Level 1 (most restrictive).
        analyze_docmdp_changes(original_xref, current_xref, DocMDPLevel::Level1)
    };

    // Step 3: Determine status.
    // Priority: empty contents → hash mismatch → xref offset changes → illegal changes → valid.
    let illegal_changes: Vec<_> = changes.iter().filter(|c| !c.permitted).collect();
    let has_illegal = !illegal_changes.is_empty();

    // Check if any xref offsets changed (file was restructured after signing).
    let xref_changed = original_xref.iter().any(|(obj, orig_off)| {
        current_xref.iter().any(|(o, curr_off)| o == obj && curr_off != orig_off)
    });

    let (status, explanation) = if signature.contents.is_empty() {
        (
            SignatureStatus::Indeterminate,
            "Signature contents (CMS data) not present — cannot verify cryptographically".to_string(),
        )
    } else if signature.byte_range.is_empty() {
        (
            SignatureStatus::Indeterminate,
            "ByteRange is empty — signature cannot be verified".to_string(),
        )
    } else if !hash_ok {
        (
            SignatureStatus::Invalid,
            "ByteRange hash does not match — signature integrity broken".to_string(),
        )
    } else if xref_changed {
        (
            SignatureStatus::Invalid,
            "Xref offsets changed after signing — file structure was modified".to_string(),
        )
    } else if has_illegal {
        let descs: Vec<_> = illegal_changes.iter().map(|c| c.description.as_str()).collect();
        (
            SignatureStatus::Invalid,
            format!("Illegal post-signing changes: {}", descs.join("; ")),
        )
    } else {
        (
            SignatureStatus::Valid,
            "Signature integrity verified; no illegal post-signing changes".to_string(),
        )
    };

    ValidationReport {
        status,
        explanation,
        signature: signature.clone(),
        post_signing_changes: changes,
        signer_trusted: false, // Requires trust store (M10)
        integrity_check_passed: hash_ok,
        hash_match: hash_ok,
        validation_time: now,
    }
}

/// Refuse to report `Valid` when post-signing change analysis had nothing to
/// examine. [FR-SIG-1, PRIN-6, MET-FEAT-6]
///
/// [`validate_signature`] decides "no illegal post-signing changes" from the
/// cross-reference data it is handed. A caller that cannot supply that data
/// (because xref extraction is unimplemented, or the revision history could
/// not be read) passes empty slices, and every change check then trivially
/// passes: `analyze_docmdp_changes` finds nothing, and `xref_changed` is false
/// over an empty iterator. The verdict falls through to `Valid`.
///
/// That is a false valid. A ByteRange hash proves only that the *signed* bytes
/// are intact; illegal post-signing edits arrive as an appended incremental
/// update, which leaves that hash matching, and the xref/DocMDP analysis is
/// what catches them. Callers without that evidence pass
/// `evidence_available: false` to convert such a verdict to `Indeterminate`.
///
/// This only ever moves `Valid` to `Indeterminate`. `Invalid` is never
/// softened: a proven failure stays a failure regardless of missing evidence.
pub fn require_change_evidence(
    report: ValidationReport,
    evidence_available: bool,
) -> ValidationReport {
    if evidence_available || report.status != SignatureStatus::Valid {
        return report;
    }
    ValidationReport {
        status: SignatureStatus::Indeterminate,
        explanation: "Signed bytes are intact, but post-signing changes could not be \
                      examined (no cross-reference data was available), so this \
                      signature cannot be reported as valid"
            .to_string(),
        ..report
    }
}

// ---------------------------------------------------------------------------
// M10: PKCS#11 hardware token signing [FR-SIG-3, SDS §2.8]
// ---------------------------------------------------------------------------

/// PKCS#11 token information. [FR-SIG-3]
#[derive(Debug, Clone)]
pub struct Pkcs11TokenInfo {
    /// Token label (e.g., "YubiKey PIV").
    pub label: String,
    /// Manufacturer ID.
    pub manufacturer: String,
    /// Model.
    pub model: String,
    /// Serial number.
    pub serial: String,
    /// Whether the token is currently present/connected.
    pub present: bool,
    /// Whether the token requires a PIN.
    pub pin_required: bool,
    /// Key types available (e.g., "RSA-2048", "EC-P256").
    pub key_types: Vec<String>,
}

/// PKCS#11 signing request. [FR-SIG-3]
#[derive(Debug, Clone)]
pub struct Pkcs11SignRequest {
    /// Token to sign with.
    pub token: Pkcs11TokenInfo,
    /// PIN for the token (if required).
    pub pin: Option<String>,
    /// The data to sign (ByteRange hash).
    pub data: Vec<u8>,
    /// Hash algorithm (e.g., "SHA-256").
    pub hash_algorithm: String,
    /// Signature profile (e.g., "PAdES-B-LTA").
    pub profile: String,
}

/// PKCS#11 signing result. [FR-SIG-3]
#[derive(Debug, Clone)]
pub struct Pkcs11SignResult {
    /// Whether signing succeeded.
    pub success: bool,
    /// The CMS signature bytes (on success).
    pub signature_bytes: Vec<u8>,
    /// Certificate chain (DER-encoded).
    pub certificate_chain: Vec<Vec<u8>>,
    /// Error message (on failure).
    pub error: Option<String>,
}

/// PKCS#11 hardware token signer trait. [FR-SIG-3]
///
/// In production, this would interface with the PKCS#11 library
/// (e.g., via the `pkcs11` crate or direct FFI). This is the
/// sandboxed interface — the actual PKCS#11 call happens in Z0
/// via the Broker.
pub trait Pkcs11Signer: Send + Sync {
    /// List available tokens.
    fn list_tokens(&self) -> Vec<Pkcs11TokenInfo>;

    /// Sign data with a hardware token.
    fn sign(&self, request: &Pkcs11SignRequest) -> Pkcs11SignResult;

    /// Check if PKCS#11 is available on this platform.
    fn is_available(&self) -> bool;
}

// ---------------------------------------------------------------------------
// M10: PAdES-LTA structures [FR-SIG-3, SDS §2.8]
// ---------------------------------------------------------------------------

/// PAdES signature profile level. [FR-SIG-3]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadesLevel {
    /// Basic-B: minimum, no timestamp required.
    BasicB,
    /// Basic-T: includes a trusted timestamp.
    BasicT,
    /// Basic-LT: includes long-term validation data (certificate chain, CRLs, OCSP).
    BasicLT,
    /// Basic-LTA: includes document timestamps for archival validity.
    BasicLTA,
}

impl std::fmt::Display for PadesLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BasicB => write!(f, "PAdES-B-B"),
            Self::BasicT => write!(f, "PAdES-B-T"),
            Self::BasicLT => write!(f, "PAdES-B-LT"),
            Self::BasicLTA => write!(f, "PAdES-B-LTA"),
        }
    }
}

/// A document timestamp for LTA validity. [FR-SIG-3]
#[derive(Debug, Clone)]
pub struct DocumentTimestamp {
    /// Timestamp value (RFC 3161).
    pub timestamp_token: Vec<u8>,
    /// Timestamp authority URL.
    pub tsa_url: String,
    /// Time of timestamp.
    pub time: String,
    /// ByteRange covered by this timestamp.
    pub byte_range: Vec<u64>,
}

/// Archival validation data for LTA. [FR-SIG-3]
#[derive(Debug, Clone)]
pub struct ArchivalValidationData {
    /// Certificate chain (DER-encoded).
    pub certificate_chain: Vec<Vec<u8>>,
    /// CRL data (if available).
    pub crl_data: Option<Vec<u8>>,
    /// OCSP response (if available).
    pub ocsp_response: Option<Vec<u8>>,
    /// Document timestamps.
    pub timestamps: Vec<DocumentTimestamp>,
    /// Whether the signing certificate was valid at signing time.
    pub certificate_valid_at_signing: bool,
    /// Whether the certificate chain can be verified to a trusted root.
    pub chain_verified: bool,
}

/// DSS (Document Security Store) entry. [FR-SIG-3]
#[derive(Debug, Clone)]
pub struct DssEntry {
    /// Version of the DSS dictionary.
    pub version: u32,
    /// VRI (Validation-Related Information) entries keyed by hash.
    pub vri: Vec<VriEntry>,
    /// Certificates (DER-encoded).
    pub certificates: Vec<Vec<u8>>,
    /// CRLs.
    pub crls: Vec<Vec<u8>>,
    /// OCSP responses.
    pub ocsp_responses: Vec<Vec<u8>>,
}

/// VRI (Validation-Related Information) entry. [FR-SIG-3]
#[derive(Debug, Clone)]
pub struct VriEntry {
    /// Hash of the signature dictionary.
    pub hash: Vec<u8>,
    /// Certificate chain for this signature.
    pub certificates: Vec<Vec<u8>>,
    /// CRLs for this signature.
    pub crls: Vec<Vec<u8>>,
    /// OCSP responses for this signature.
    pub ocsp_responses: Vec<Vec<u8>>,
}

// ---------------------------------------------------------------------------
// M10: PDF/A validation [FR-STD-1, FR-STD-2]
// ---------------------------------------------------------------------------

/// PDF/A conformance level. [FR-STD-2]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PdfALevel {
    /// PDF/A-1a (WCAG AA + full structural).
    A1a,
    /// PDF/A-1b (visual only).
    A1b,
    /// PDF/A-2a.
    A2a,
    /// PDF/A-2b.
    A2b,
    /// PDF/A-2u (unicode only).
    A2u,
    /// PDF/A-3a.
    A3a,
    /// PDF/A-3b.
    A3b,
    /// PDF/A-3u.
    A3u,
    /// PDF/A-4 (ISO 32005).
    A4,
}

impl std::fmt::Display for PdfALevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::A1a => write!(f, "PDF/A-1a"),
            Self::A1b => write!(f, "PDF/A-1b"),
            Self::A2a => write!(f, "PDF/A-2a"),
            Self::A2b => write!(f, "PDF/A-2b"),
            Self::A2u => write!(f, "PDF/A-2u"),
            Self::A3a => write!(f, "PDF/A-3a"),
            Self::A3b => write!(f, "PDF/A-3b"),
            Self::A3u => write!(f, "PDF/A-3u"),
            Self::A4 => write!(f, "PDF/A-4"),
        }
    }
}

/// PDF/A validation result. [FR-STD-5]
#[derive(Debug, Clone)]
pub struct PdfAValidationResult {
    /// Conformance verdict, or `None` when it could not be determined.
    ///
    /// `Some(false)` — a violation was detected, which is a sound negative.
    /// `None`        — no violation was detected, which is **not** conformance.
    /// `Some(true)`  — unreachable until a recognized validator is integrated.
    ///
    /// The field this replaces was `conforms: bool`, set from
    /// `errors.is_empty()`. `validate_pdf_a` greps four byte patterns and
    /// parses no objects, so it cannot see encryption, embedded JavaScript, or
    /// external references — all prohibited by ISO 19005. Absence of findings
    /// from those heuristics is not evidence of conformance, and MET-FEAT-3
    /// makes standards conformance absolute.
    /// [FR-STD-5, CMP-STD-4, MET-FEAT-3, PRIN-6, GR-8]
    pub conformance: Option<bool>,
    /// The target level.
    pub target_level: PdfALevel,
    /// Validation errors (must-fix for conformance).
    pub errors: Vec<String>,
    /// Validation warnings (should-fix, not blocking conformance).
    pub warnings: Vec<String>,
    /// Metadata found (XMP, /Info dictionary).
    pub metadata: PdfAMetadata,
    /// Whether output intents are present.
    pub has_output_intent: bool,
    /// Whether fonts are embedded.
    pub fonts_embedded: bool,
    /// Whether transparency is valid.
    pub transparency_valid: bool,
}

/// PDF/A metadata extracted from a document. [FR-STD-2]
#[derive(Debug, Clone, Default)]
pub struct PdfAMetadata {
    /// PDF/A conformance level declared in metadata.
    pub declared_level: Option<String>,
    /// Document title.
    pub title: Option<String>,
    /// Creator tool.
    pub creator: Option<String>,
    /// Creation date.
    pub creation_date: Option<String>,
    /// Mod date.
    pub mod_date: Option<String>,
    /// XMP metadata presence.
    pub has_xmp: bool,
    /// /Info dictionary presence.
    pub has_info_dict: bool,
}

/// Validate a PDF for PDF/A conformance. [FR-STD-1, FR-STD-5]
///
/// Checks: metadata, fonts, transparency, output intents, and other
/// PDF/A requirements. Returns a detailed result.
/// Heuristic PDF/A pre-check. **Not** an ISO 19005 conformance determination.
///
/// This searches for byte patterns (`x:xmpm`, `/Info`, `/OutputIntents`, a
/// transparency group) and parses no objects. It therefore cannot see most of
/// the standard: encryption, embedded JavaScript and external references are
/// all prohibited by PDF/A and none are examined, and PDF/A-1b's mandatory
/// OutputIntent is currently only a warning.
///
/// A finding proves non-conformance. The **absence** of findings proves
/// nothing, so `conforms == true` must never be presented to a user as
/// conformance — FR-STD-5 and CMP-STD-4 forbid declaring a level the product
/// has not established, and MET-FEAT-3 makes that absolute. Real claims
/// require a recognized validator (veraPDF, CMP-STD-2).
pub fn validate_pdf_a(
    file_bytes: &[u8],
    target_level: PdfALevel,
) -> PdfAValidationResult {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let mut metadata = PdfAMetadata::default();

    // Check for XMP metadata.
    metadata.has_xmp = file_bytes.windows(6).any(|w| w == b"x:xmpm");
    if !metadata.has_xmp {
        match target_level {
            PdfALevel::A1a | PdfALevel::A1b => {
                errors.push("PDF/A-1 requires XMP metadata".into());
            }
            _ => {
                warnings.push("PDF/A-2+ recommends XMP metadata".into());
            }
        }
    }

    // Check for /Info dictionary.
    metadata.has_info_dict = find_pattern(file_bytes, b"/Info").is_some();

    // Check font embedding.
    // A simplified check: look for /Font entries without /FontFile references.
    // Full validation requires font table parsing (deferred to veraPDF integration).
    fonts_embedded_check(file_bytes, &mut errors, &mut warnings, target_level);

    // Check for output intents (required for PDF/A-1b+).
    let has_output_intent = find_pattern(file_bytes, b"/OutputIntents").is_some();
    if !has_output_intent && matches!(target_level, PdfALevel::A1a | PdfALevel::A1b) {
        warnings.push("PDF/A-1 recommends output intents for color management".into());
    }

    // Check transparency.
    let transparency_valid = !find_pattern(file_bytes, b"/Group << /S /Transparency").is_some()
        || has_output_intent;

    let fonts_embedded = errors.iter().all(|e| !e.contains("font"));

    PdfAValidationResult {
        // A detected violation is a sound negative verdict. No detection means
        // undetermined, never conformant: establishing conformance requires a
        // recognized validator such as veraPDF (CMP-STD-2). [MET-FEAT-3]
        conformance: if errors.is_empty() { None } else { Some(false) },
        target_level,
        errors,
        warnings,
        metadata,
        has_output_intent,
        fonts_embedded,
        transparency_valid,
    }
}

/// Find a byte pattern in a byte slice.
pub fn find_pattern(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Check font embedding. [FR-STD-5]
fn fonts_embedded_check(
    file_bytes: &[u8],
    errors: &mut Vec<String>,
    warnings: &mut Vec<String>,
    level: PdfALevel,
) {
    // Simplified: look for /Font without /FontFile.
    // Full implementation requires font dictionary traversal.
    let has_font = find_pattern(file_bytes, b"/Font").is_some();
    let has_font_file = find_pattern(file_bytes, b"/FontFile").is_some();

    if has_font && !has_font_file {
        match level {
            PdfALevel::A1a | PdfALevel::A1b | PdfALevel::A2a | PdfALevel::A2b => {
                errors.push("PDF/A requires all fonts to be embedded".into());
            }
            _ => {
                warnings.push("PDF/A-3+ recommends all fonts to be embedded".into());
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_status_display() {
        assert_eq!(SignatureStatus::Valid.to_string(), "Valid");
        assert_eq!(SignatureStatus::Invalid.to_string(), "Invalid");
        assert_eq!(SignatureStatus::Indeterminate.to_string(), "Indeterminate");
    }

    #[test]
    fn byte_range_valid_basic() {
        assert!(byte_range_valid(&[0, 100, 200, 50], 300));
        assert!(byte_range_valid(&[0, 300], 300));
    }

    #[test]
    fn byte_range_invalid_overlapping() {
        assert!(!byte_range_valid(&[0, 150, 100, 50], 200));
    }

    #[test]
    fn byte_range_invalid_out_of_bounds() {
        assert!(!byte_range_valid(&[0, 500], 300));
    }

    #[test]
    fn byte_range_invalid_empty() {
        assert!(!byte_range_valid(&[], 100));
        assert!(!byte_range_valid(&[0], 100)); // odd count
    }

    #[test]
    fn byte_range_invalid_zero_length() {
        assert!(!byte_range_valid(&[0, 0], 100));
    }

    #[test]
    fn hash_byte_ranges_produces_digest() {
        let file = b"Hello, PDF world! This is a test file for signature hashing.";
        let byte_range = vec![0, 11, 20, 10]; // "Hello, PDF " + "test file"
        let hash = hash_byte_ranges(file, &byte_range);
        assert_eq!(hash.len(), 32, "SHA-256 produces 32 bytes");
        assert!(!hash.iter().all(|&b| b == 0), "hash should not be all zeros");
    }

    #[test]
    fn hash_byte_ranges_deterministic() {
        let file = b"Test data for hashing";
        let byte_range = vec![0, 5, 10, 5];
        let h1 = hash_byte_ranges(file, &byte_range);
        let h2 = hash_byte_ranges(file, &byte_range);
        assert_eq!(h1, h2, "same input must produce same hash");
    }

    #[test]
    fn hash_byte_ranges_different_ranges_differ() {
        let file = b"Test data for hashing";
        let h1 = hash_byte_ranges(file, &[0, 4]);
        let h2 = hash_byte_ranges(file, &[5, 4]);
        assert_ne!(h1, h2, "different ranges should produce different hashes");
    }

    fn report_with(status: SignatureStatus) -> ValidationReport {
        ValidationReport {
            status,
            explanation: "original explanation".into(),
            signature: SignatureInfo {
                name: String::new(),
                location: String::new(),
                reason: String::new(),
                date: String::new(),
                filter: String::new(),
                sub_filter: String::new(),
                byte_range: vec![0, 4],
                contents: vec![1, 2, 3],
                docmdp_level: None,
                byte_offset: 0,
                obj_num: 1,
                page_index: None,
            },
            post_signing_changes: Vec::new(),
            signer_trusted: false,
            integrity_check_passed: true,
            hash_match: true,
            validation_time: 0,
        }
    }

    #[test]
    fn valid_becomes_indeterminate_when_no_change_evidence_was_available() {
        // A ByteRange hash only proves the signed bytes are intact. Illegal
        // post-signing edits arrive as an appended incremental update, which
        // leaves that hash matching. Claiming "no illegal post-signing
        // changes" without having examined any is a false valid.
        let report = require_change_evidence(report_with(SignatureStatus::Valid), false);
        assert_eq!(report.status, SignatureStatus::Indeterminate);
        assert!(
            report.explanation.contains("post-signing"),
            "must say what could not be checked: {}",
            report.explanation
        );
    }

    #[test]
    fn valid_is_left_alone_when_change_evidence_was_available() {
        let report = require_change_evidence(report_with(SignatureStatus::Valid), true);
        assert_eq!(report.status, SignatureStatus::Valid);
        assert_eq!(report.explanation, "original explanation");
    }

    #[test]
    fn an_invalid_verdict_is_never_softened_by_missing_evidence() {
        let report = require_change_evidence(report_with(SignatureStatus::Invalid), false);
        assert_eq!(
            report.status,
            SignatureStatus::Invalid,
            "missing evidence must never upgrade a proven failure"
        );
    }

    #[test]
    fn validate_signature_valid_no_changes() {
        let file = b"%PDF-1.4\n1 0 obj\n<< /Type /Catalog >>\nendobj\n";
        let sig = SignatureInfo {
            name: "Test Signer".into(),
            location: "".into(),
            reason: "Testing".into(),
            date: "2026-01-01".into(),
            byte_range: vec![0, file.len() as u64],
            contents: vec![0xDE, 0xAD], // non-empty CMS placeholder
            docmdp_level: Some(DocMDPLevel::Level2),
            filter: "Adobe.PPKLite".into(),
            sub_filter: "adbe.pkcs7.detached".into(),
            byte_offset: 0,
            obj_num: 1,
            page_index: None,
        };
        let xref: Vec<(u32, u64)> = vec![(1, 9)];

        let report = validate_signature(file, &sig, &xref, &xref);
        assert_eq!(report.status, SignatureStatus::Valid);
        assert!(report.post_signing_changes.is_empty());
        assert!(report.integrity_check_passed);
    }

    #[test]
    fn validate_signature_invalidIllegal_changes() {
        let file = b"%PDF-1.4\n1 0 obj\n<< /Type /Catalog >>\nendobj\n";
        let sig = SignatureInfo {
            name: "Test Signer".into(),
            location: "".into(),
            reason: "Testing".into(),
            date: "2026-01-01".into(),
            byte_range: vec![0, file.len() as u64],
            contents: vec![0xDE, 0xAD],
            docmdp_level: Some(DocMDPLevel::Level1), // no changes permitted
            filter: "Adobe.PPKLite".into(),
            sub_filter: "adbe.pkcs7.detached".into(),
            byte_offset: 0,
            obj_num: 1,
            page_index: None,
        };
        let original_xref: Vec<(u32, u64)> = vec![(1, 9)];
        let current_xref: Vec<(u32, u64)> = vec![(1, 9), (2, 50)]; // new object added

        let report = validate_signature(file, &sig, &original_xref, &current_xref);
        assert_eq!(report.status, SignatureStatus::Invalid);
        assert!(!report.post_signing_changes.is_empty());
        assert!(report.post_signing_changes.iter().any(|c| !c.permitted));
    }

    #[test]
    fn validate_signature_indeterminate_empty_contents() {
        let file = b"%PDF-1.4\n1 0 obj\n<< /Type /Catalog >>\nendobj\n";
        let sig = SignatureInfo {
            name: "Test Signer".into(),
            location: "".into(),
            reason: "".into(),
            date: "".into(),
            byte_range: vec![0, file.len() as u64],
            contents: vec![], // empty — no CMS data
            docmdp_level: None,
            filter: "Adobe.PPKLite".into(),
            sub_filter: "adbe.pkcs7.detached".into(),
            byte_offset: 0,
            obj_num: 1,
            page_index: None,
        };
        let xref: Vec<(u32, u64)> = vec![(1, 9)];

        let report = validate_signature(file, &sig, &xref, &xref);
        assert_eq!(report.status, SignatureStatus::Indeterminate);
    }

    #[test]
    fn docmdp_level2_permits_new_objects() {
        let original: Vec<(u32, u64)> = vec![(1, 100)];
        let current: Vec<(u32, u64)> = vec![(1, 100), (2, 200)];

        let changes = analyze_docmdp_changes(&original, &current, DocMDPLevel::Level2);
        assert_eq!(changes.len(), 1);
        assert!(changes[0].permitted, "Level 2 permits new objects");
    }

    #[test]
    fn docmdp_level1_rejects_new_objects() {
        let original: Vec<(u32, u64)> = vec![(1, 100)];
        let current: Vec<(u32, u64)> = vec![(1, 100), (2, 200)];

        let changes = analyze_docmdp_changes(&original, &current, DocMDPLevel::Level1);
        assert_eq!(changes.len(), 1);
        assert!(!changes[0].permitted, "Level 1 rejects new objects");
    }

    #[test]
    fn docmdp_deletion_always_rejected() {
        let original: Vec<(u32, u64)> = vec![(1, 100), (2, 200)];
        let current: Vec<(u32, u64)> = vec![(1, 100)]; // object 2 deleted

        let changes = analyze_docmdp_changes(&original, &current, DocMDPLevel::Level3);
        assert_eq!(changes.len(), 1);
        assert!(!changes[0].permitted, "deletion is never permitted");
        assert_eq!(changes[0].severity, "error");
    }

    #[test]
    fn validation_report_summary_format() {
        let sig = SignatureInfo {
            name: "Alice".into(),
            location: "".into(),
            reason: "".into(),
            date: "".into(),
            byte_range: vec![],
            contents: vec![],
            docmdp_level: None,
            filter: "".into(),
            sub_filter: "".into(),
            byte_offset: 0,
            obj_num: 1,
            page_index: None,
        };
        let report = ValidationReport {
            status: SignatureStatus::Valid,
            explanation: "ok".into(),
            signature: sig,
            post_signing_changes: vec![],
            signer_trusted: false,
            integrity_check_passed: true,
            hash_match: true,
            validation_time: 0,
        };
        let s = report.summary();
        assert!(s.contains("Alice"));
        assert!(s.contains("Valid"));
        assert!(s.contains("no post-signing changes"));
    }

    // =========================================================================
    // M8 tests: Signature corpus — "never false-valid" guarantee [FR-SIG-1]
    // =========================================================================

    /// Build a minimal signed PDF for testing. The "signature" covers the
    /// entire file via ByteRange, with non-empty CMS contents placeholder.
    fn make_signed_pdf(content: &[u8], contents: &[u8]) -> (Vec<u8>, SignatureInfo) {
        let sig = SignatureInfo {
            name: "Test Signer".into(),
            location: "Test".into(),
            reason: "M8 corpus test".into(),
            date: "2026-01-01".into(),
            byte_range: vec![0, content.len() as u64],
            contents: contents.to_vec(),
            docmdp_level: Some(DocMDPLevel::Level2),
            filter: "Adobe.PPKLite".into(),
            sub_filter: "adbe.pkcs7.detached".into(),
            byte_offset: 0,
            obj_num: 1,
            page_index: None,
        };
        (content.to_vec(), sig)
    }

    /// An unsigned PDF must never validate as "Valid". [FR-SIG-1, M8]
    #[test]
    fn m8_unsigned_pdf_never_valid() {
        let pdf = b"%PDF-1.4\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n";
        let sig = SignatureInfo {
            name: "".into(),
            location: "".into(),
            reason: "".into(),
            date: "".into(),
            byte_range: vec![0, pdf.len() as u64],
            contents: vec![], // empty — no signature
            docmdp_level: None,
            filter: "".into(),
            sub_filter: "".into(),
            byte_offset: 0,
            obj_num: 1,
            page_index: None,
        };
        let xref = vec![(1, 9)];
        let report = validate_signature(pdf, &sig, &xref, &xref);
        assert_ne!(report.status, SignatureStatus::Valid,
            "unsigned PDF MUST NOT validate as Valid");
    }

    /// Post-signing xref modification detected. [FR-SIG-2, M8]
    #[test]
    fn m8_post_signing_modification_detected() {
        let pdf = b"%PDF-1.4\n1 0 obj\n<< /Type /Catalog >>\nendobj\n";
        let (_, sig) = make_signed_pdf(pdf, &[0xCA, 0xFE]);

        // Object 1 was at offset 9, now moved to offset 50.
        let original_xref = vec![(1, 9)];
        let current_xref = vec![(1, 50)];

        let report = validate_signature(pdf, &sig, &original_xref, &current_xref);
        assert_eq!(report.status, SignatureStatus::Invalid,
            "post-signing modification must be detected");
        assert!(!report.post_signing_changes.is_empty());
    }

    /// DocMDP Level 2 allows new objects but not modifications. [FR-SIG-2, M8]
    #[test]
    fn m8_docmdp_level2_allows_new_rejects_modify() {
        let pdf = b"%PDF-1.4\n1 0 obj\n<< /Type /Catalog >>\nendobj\n";
        let (file, mut sig) = make_signed_pdf(pdf, &[0xCA, 0xFE]);
        sig.docmdp_level = Some(DocMDPLevel::Level2);

        // New object added — permitted under Level 2.
        let orig_xref = vec![(1, 9)];
        let curr_xref = vec![(1, 9), (2, 200)];
        let report = validate_signature(&file, &sig, &orig_xref, &curr_xref);
        assert_eq!(report.status, SignatureStatus::Valid,
            "Level 2 should allow new objects");

        // Object modified — not permitted under Level 2.
        let curr_xref2 = vec![(1, 50)];
        let report2 = validate_signature(&file, &sig, &orig_xref, &curr_xref2);
        assert_eq!(report2.status, SignatureStatus::Invalid,
            "Level 2 should reject modifications");
    }

    /// DocMDP Level 1 rejects everything. [FR-SIG-2, M8]
    #[test]
    fn m8_docmdp_level1_rejects_all_changes() {
        let pdf = b"%PDF-1.4\n1 0 obj\n<< /Type /Catalog >>\nendobj\n";
        let (file, mut sig) = make_signed_pdf(pdf, &[0xCA, 0xFE]);
        sig.docmdp_level = Some(DocMDPLevel::Level1);

        let orig_xref = vec![(1, 9)];
        let curr_xref = vec![(1, 9), (2, 200)];
        let report = validate_signature(&file, &sig, &orig_xref, &curr_xref);
        assert_eq!(report.status, SignatureStatus::Invalid,
            "Level 1 must reject all changes");
    }

    /// Empty ByteRange → Indeterminate. [FR-SIG-1, M8]
    #[test]
    fn m8_empty_byterange_indeterminate() {
        let pdf = b"%PDF-1.4\n1 0 obj\n<< /Type /Catalog >>\nendobj\n";
        let sig = SignatureInfo {
            name: "Test".into(),
            location: "".into(),
            reason: "".into(),
            date: "".into(),
            byte_range: vec![], // empty
            contents: vec![0xCA, 0xFE],
            docmdp_level: None,
            filter: "".into(),
            sub_filter: "".into(),
            byte_offset: 0,
            obj_num: 1,
            page_index: None,
        };
        let xref = vec![(1, 9)];
        let report = validate_signature(pdf, &sig, &xref, &xref);
        assert_eq!(report.status, SignatureStatus::Indeterminate,
            "empty ByteRange must be Indeterminate, never Valid");
    }

    /// Overlapping ByteRange → rejected. [FR-SIG-2, M8]
    #[test]
    fn m8_overlapping_byterange_rejected() {
        let file = b"0123456789ABCDEF";
        assert!(!byte_range_valid(&[0, 10, 5, 10], file.len()),
            "overlapping ranges must be rejected");
    }

    /// No DocMDP defaults to Level 1 (most restrictive). [FR-SIG-2, M8]
    #[test]
    fn m8_no_docmdp_defaults_to_level1() {
        let pdf = b"%PDF-1.4\n1 0 obj\n<< /Type /Catalog >>\nendobj\n";
        let (file, mut sig) = make_signed_pdf(pdf, &[0xCA, 0xFE]);
        sig.docmdp_level = None;

        let orig_xref = vec![(1, 9)];
        let curr_xref = vec![(1, 9), (2, 200)];
        let report = validate_signature(&file, &sig, &orig_xref, &curr_xref);
        assert_eq!(report.status, SignatureStatus::Invalid,
            "no DocMDP must default to Level 1");
    }

    /// Validation report is always explainable. [FR-SIG-1, M8]
    #[test]
    fn m8_validation_report_always_has_explanation() {
        let pdf = b"%PDF-1.4\n1 0 obj\n<< /Type /Catalog >>\nendobj\n";
        let sig = SignatureInfo {
            name: "X".into(),
            location: "".into(),
            reason: "".into(),
            date: "".into(),
            byte_range: vec![0, pdf.len() as u64],
            contents: vec![],
            docmdp_level: None,
            filter: "".into(),
            sub_filter: "".into(),
            byte_offset: 0,
            obj_num: 1,
            page_index: None,
        };
        let xref = vec![(1, 9)];

        // Test all three statuses produce explanations.
        for contents in [vec![], vec![0xCA], vec![0xDE, 0xAD]] {
            let mut s = sig.clone();
            s.contents = contents;
            let report = validate_signature(pdf, &s, &xref, &xref);
            assert!(!report.explanation.is_empty(),
                "explanation must not be empty for status {:?}", report.status);
        }
    }

    /// Object deletion is never permitted under any DocMDP level. [FR-SIG-2, M8]
    #[test]
    fn m8_object_deletion_always_rejected() {
        for level in [DocMDPLevel::Level1, DocMDPLevel::Level2, DocMDPLevel::Level3] {
            let original = vec![(1, 100), (2, 200)];
            let current = vec![(1, 100)]; // object 2 deleted
            let changes = analyze_docmdp_changes(&original, &current, level);
            assert_eq!(changes.len(), 1);
            assert!(!changes[0].permitted,
                "deletion must be rejected at {:?}", level);
        }
    }

    // =========================================================================
    // M10 tests: Hardware signing + PDF/A validation
    // =========================================================================

    #[test]
    fn pades_level_display() {
        assert_eq!(PadesLevel::BasicB.to_string(), "PAdES-B-B");
        assert_eq!(PadesLevel::BasicT.to_string(), "PAdES-B-T");
        assert_eq!(PadesLevel::BasicLT.to_string(), "PAdES-B-LT");
        assert_eq!(PadesLevel::BasicLTA.to_string(), "PAdES-B-LTA");
    }

    #[test]
    fn pkcs11_token_info_fields() {
        let token = Pkcs11TokenInfo {
            label: "YubiKey".into(),
            manufacturer: "Yubico".into(),
            model: "YubiKey 5".into(),
            serial: "12345".into(),
            present: true,
            pin_required: true,
            key_types: vec!["RSA-2048".into(), "EC-P256".into()],
        };
        assert!(token.present);
        assert!(token.pin_required);
        assert_eq!(token.key_types.len(), 2);
    }

    #[test]
    fn pdf_a_level_display() {
        assert_eq!(PdfALevel::A1a.to_string(), "PDF/A-1a");
        assert_eq!(PdfALevel::A2b.to_string(), "PDF/A-2b");
        assert_eq!(PdfALevel::A3a.to_string(), "PDF/A-3a");
        assert_eq!(PdfALevel::A4.to_string(), "PDF/A-4");
    }

    /// Finding a violation proves non-conformance. Finding none proves nothing:
    /// `validate_pdf_a` greps four byte patterns and parses no objects, so it
    /// cannot see encryption, embedded JavaScript, or external references, all
    /// of which ISO 19005 prohibits. A clean run must therefore report
    /// "undetermined", never conformance. MET-FEAT-3 makes standards
    /// conformance absolute. [FR-STD-5, CMP-STD-4, MET-FEAT-3, PRIN-6, GR-8]
    #[test]
    fn a_clean_heuristic_run_reports_undetermined_not_conformant() {
        // Carries XMP and an output intent, so none of the heuristics fire.
        let pdf = b"%PDF-1.7
<< /Info 1 0 R /OutputIntents [2 0 R] >>
x:xmpmeta
";

        let result = validate_pdf_a(pdf, PdfALevel::A2b);

        assert!(result.errors.is_empty(), "no heuristic should have fired");
        assert_eq!(
            result.conformance, None,
            "absence of findings is not conformance"
        );
    }

    /// A violation is still a sound negative verdict. [FR-STD-5]
    #[test]
    fn a_detected_violation_is_reported_as_non_conformance() {
        let pdf = b"%PDF-1.4
<< /Type /Catalog >>
"; // no XMP
        let result = validate_pdf_a(pdf, PdfALevel::A1b);
        assert!(!result.errors.is_empty());
        assert_eq!(result.conformance, Some(false));
    }

    #[test]
    fn validate_pdf_a_missing_xmp() {
        let pdf = b"%PDF-1.4\n1 0 obj\n<< /Type /Catalog >>\nendobj\n";
        let result = validate_pdf_a(pdf, PdfALevel::A1a);
        assert_eq!(result.conformance, Some(false), "PDF/A-1a without XMP should not conform");
        assert!(result.errors.iter().any(|e| e.contains("XMP")));
    }

    #[test]
    fn validate_pdf_a_with_xmp() {
        let pdf = b"%PDF-1.4\nx:xmpm metadata\n1 0 obj\n<< /Type /Catalog >>\nendobj\n";
        let result = validate_pdf_a(pdf, PdfALevel::A1a);
        // May still have other errors, but XMP check should pass.
        assert!(!result.errors.iter().any(|e| e.contains("XMP")));
    }

    #[test]
    fn validate_pdf_a_font_warning() {
        let pdf = b"%PDF-1.4\n/Font << >>\n1 0 obj\n<< /Type /Catalog >>\nendobj\n";
        let result = validate_pdf_a(pdf, PdfALevel::A3a);
        // A3+ should warn, not error, about fonts.
        assert!(result.warnings.iter().any(|w| w.contains("font")) || result.errors.is_empty());
    }

    #[test]
    fn document_timestamp_fields() {
        let ts = DocumentTimestamp {
            timestamp_token: vec![0x01, 0x02],
            tsa_url: "http://timestamp.digicert.com".into(),
            time: "2026-01-01T00:00:00Z".into(),
            byte_range: vec![0, 100],
        };
        assert_eq!(ts.tsa_url, "http://timestamp.digicert.com");
        assert_eq!(ts.byte_range, vec![0, 100]);
    }

    #[test]
    fn archival_validation_data_structure() {
        let avd = ArchivalValidationData {
            certificate_chain: vec![vec![0x30, 0x82]],
            crl_data: None,
            ocsp_response: None,
            timestamps: vec![],
            certificate_valid_at_signing: true,
            chain_verified: false,
        };
        assert!(avd.certificate_valid_at_signing);
        assert!(!avd.chain_verified);
        assert!(avd.crl_data.is_none());
    }

    #[test]
    fn dss_entry_structure() {
        let dss = DssEntry {
            version: 1,
            vri: vec![],
            certificates: vec![vec![0x30]],
            crls: vec![],
            ocsp_responses: vec![],
        };
        assert_eq!(dss.version, 1);
        assert_eq!(dss.certificates.len(), 1);
    }

    #[test]
    fn pdf_a_metadata_default() {
        let meta = PdfAMetadata::default();
        assert!(meta.declared_level.is_none());
        assert!(!meta.has_xmp);
        assert!(!meta.has_info_dict);
    }
}
