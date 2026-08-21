//! Corrupt-file corpus: every tolerated deviation must surface, and none may crash.
//!
//! SDS §10.6 names this stratum directly — "corrupt-file corpora (asserting
//! graceful leniency, never a crash)" — and GR-8 requires tolerated deviations
//! to reach the user through diagnostics rather than a false success. ADR-005
//! opens on "rendering fidelity against decades of malformed real-world files".
//!
//! Before this suite, `pdf-cos` recorded seven distinct leniency event kinds and
//! **no test asserted that any of them ever fired**. The only leniency
//! assertion in the crate was `assert!(ds.leniency.is_empty())` on the happy
//! path, and `tools/corpus-diff/fixtures/` holds two files, both named `valid`,
//! so the corpus gate could not exercise a single repair path either.
//!
//! Every fixture here is derived from a valid document this repository already
//! owns, so nothing is committed that carries provenance or confidentiality
//! questions (GIT-5).
//!
//! [SDS §10.6, ADR-005, ADR-022, T-2, T-4, MET-FEAT-1, GR-8, PRIN-1, PRIN-6]

use std::io::Write;

use pdf_cos::scan::scan_structure;

/// A structurally valid one-page document, byte-identical in shape to the
/// fixture `pdf-cos`'s own unit test uses.
const VALID: &[u8] = b"%PDF-1.4\n\
1 0 obj\n\
<</Type /Catalog /Pages 2 0 R>>\n\
endobj\n\
2 0 obj\n\
<</Type /Pages /Kids [3 0 R] /Count 1>>\n\
endobj\n\
3 0 obj\n\
<</Type /Page /Parent 2 0 R /MediaBox [0 0 612 792]>>\n\
endobj\n\
xref\n\
0 4\n\
0000000000 65535 f \n\
0000000009 00000 n \n\
0000000056 00000 n \n\
0000000111 00000 n \n\
trailer\n\
<</Size 4 /Root 1 0 R>>\n\
startxref\n\
180\n\
%%EOF";

/// Write `bytes` to a uniquely named temp file and scan it through the real
/// public entry point, so the mmap path is exercised rather than bypassed.
fn scan(bytes: &[u8], label: &str) -> Result<pdf_cos::scan::DocumentStructure, String> {
    let path = std::env::temp_dir().join(format!(
        "pdf-platform-leniency-{}-{}-{}.pdf",
        std::process::id(),
        label,
        bytes.len()
    ));
    let mut f = std::fs::File::create(&path).expect("create fixture");
    f.write_all(bytes).expect("write fixture");
    f.sync_all().ok();
    drop(f);

    let result = scan_structure(&path).map_err(|e| e.to_string());
    std::fs::remove_file(&path).ok();
    result
}

fn kinds(ds: &pdf_cos::scan::DocumentStructure) -> Vec<&'static str> {
    ds.leniency.iter().map(|e| e.kind).collect()
}

/// The baseline must stay clean, or every assertion below means nothing.
#[test]
fn the_valid_baseline_reports_no_leniency() {
    let ds = scan(VALID, "valid").expect("valid document scans");
    assert_eq!(ds.page_count, 1);
    assert!(
        ds.leniency.is_empty(),
        "baseline must be clean, got {:?}",
        kinds(&ds)
    );
}

/// A document that does not start with `%PDF-` is repaired, not rejected — and
/// the deviation has to be visible. [GR-8, FR-DIAG-1]
#[test]
fn a_missing_header_surfaces_as_leniency() {
    // Exactly eight bytes replace "%PDF-1.4", so every xref offset in the
    // file stays correct and the only defect is the missing marker.
    let mut bytes = b"GARBAGE!".to_vec();
    bytes.extend_from_slice(&VALID[8..]);
    assert_eq!(bytes.len(), VALID.len(), "offsets must be preserved");

    match scan(&bytes, "no-header") {
        Ok(ds) => assert!(
            kinds(&ds).contains(&"missing-pdf-header"),
            "expected missing-pdf-header, got {:?}",
            kinds(&ds)
        ),
        Err(e) => panic!("a missing header must be tolerated, not fatal: {e}"),
    }
}

/// An xref table whose entries stop early must be reported, never silently
/// treated as complete.
#[test]
fn a_truncated_xref_table_surfaces_as_leniency() {
    // "0 4" -> "0 9" declares nine entries where four exist. Same byte length,
    // so every offset in the file remains valid and the table itself is the
    // only defect.
    let bytes: Vec<u8> = String::from_utf8_lossy(VALID)
        .replacen("xref\n0 4\n", "xref\n0 9\n", 1)
        .into_bytes();
    assert_eq!(bytes.len(), VALID.len(), "offsets must be preserved");

    match scan(&bytes, "short-xref") {
        Ok(ds) => assert!(
            kinds(&ds).contains(&"xref-truncated"),
            "a table declaring more entries than it holds must surface \
             xref-truncated, got {:?}",
            kinds(&ds)
        ),
        Err(e) => panic!("a short xref table must be tolerated, not fatal: {e}"),
    }
}

/// SDS §10.4 and ADR-006 put qpdf-style xref reconstruction in this layer, and
/// `pdf_cos::xref::reconstruct_xref` implements it — documented as "when the
/// xref table is damaged or missing, scan the file for all object definitions
/// and build an xref from them", with its own unit test.
///
/// **Its only caller is that unit test.** Nothing on the scan path invokes it,
/// so a document with an unusable `startxref` fails to open with "malformed
/// xref table" instead of being repaired, and the `xref-reconstructed` leniency
/// event can never fire in production.
///
/// Ignored rather than deleted or inverted: AI-7 forbids a test that asserts
/// the current behaviour is correct, and this records what SDS §10.4 requires
/// so that wiring reconstruction in has a waiting assertion.
/// [SDS §10.4, ADR-006, FR-VIEW-2, GR-8, AI-7]
#[test]
#[ignore = "reconstruct_xref has no production caller; SDS §10.4 recovery is not wired into the scan path"]
fn a_bogus_startxref_forces_recorded_reconstruction() {
    // "180" -> "999" points past the end of the file. Same byte length.
    let bytes: Vec<u8> = String::from_utf8_lossy(VALID)
        .replacen("startxref\n180\n", "startxref\n999\n", 1)
        .into_bytes();
    assert_eq!(bytes.len(), VALID.len(), "offsets must be preserved");

    match scan(&bytes, "bad-startxref") {
        Ok(ds) => {
            assert!(
                kinds(&ds).contains(&"xref-reconstructed"),
                "reconstruction must be recorded, got {:?}",
                kinds(&ds)
            );
            assert_eq!(ds.page_count, 1, "reconstruction should recover the page");
        }
        Err(e) => panic!("a damaged startxref should reconstruct, not fail: {e}"),
    }
}

/// Truncation sweep. Every prefix of a valid document must either scan or fail
/// cleanly. A panic here is a crash on untrusted input, which PRIN-1 and T-4
/// rule out regardless of how malformed the bytes are.
#[test]
fn no_prefix_of_a_valid_document_can_crash_the_scanner() {
    for len in (0..VALID.len()).step_by(7) {
        let _ = scan(&VALID[..len], &format!("prefix{len}"));
    }
    // Also every byte of the trailer region, where the parser does its
    // arithmetic on offsets read out of the file.
    let tail_start = VALID.len().saturating_sub(120);
    for len in tail_start..VALID.len() {
        let _ = scan(&VALID[..len], &format!("tail{len}"));
    }
}

/// Single-byte corruption sweep over the structural tail. Offsets and lengths
/// are parsed out of these bytes, so this is where an unchecked subtraction or
/// slice would show up. [T-4]
#[test]
fn single_byte_corruption_in_the_trailer_cannot_crash_the_scanner() {
    let start = VALID.len().saturating_sub(120);
    for i in start..VALID.len() {
        let mut bytes = VALID.to_vec();
        bytes[i] = b'\xff';
        let _ = scan(&bytes, &format!("byte{i}"));
    }
}

/// An empty file and a file of pure noise are the degenerate ends of the
/// corpus; neither may panic.
#[test]
fn degenerate_inputs_are_refused_without_panicking() {
    let _ = scan(b"", "empty");
    let _ = scan(b"\x00\x00\x00\x00", "nulls");
    let _ = scan(&vec![b'A'; 4096], "noise");
    let _ = scan(b"%PDF-1.4\n", "header-only");
}
