//! Minimal structural scanner. [ADR-006, SDS §14 M0, FR-DIAG-2]
//!
//! Reads classic xref tables only. Compressed xref (PDF 1.5+), encryption,
//! and linearized hint streams are deferred to M1.

use std::path::Path;
use crate::leniency::LeniencyEvent;

/// Structural summary of a PDF document produced by the minimal M0 scanner.
#[derive(Debug)]
pub struct DocumentStructure {
    pub page_count: u32,
    pub has_acroform: bool,
    pub has_xfa: bool,
    pub has_js: bool,
    pub sig_count: u32,
    pub leniency: Vec<LeniencyEvent>,
}

/// Fatal scanner errors (not tolerable leniency events).
#[derive(Debug)]
pub enum ScanError {
    Io(std::io::Error),
    NoStartxref,
    NoTrailer,
    NoRoot,
    MalformedXref,
}

impl std::fmt::Display for ScanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScanError::Io(e)        => write!(f, "I/O error: {e}"),
            ScanError::NoStartxref  => write!(f, "no startxref marker found"),
            ScanError::NoTrailer    => write!(f, "no trailer dictionary found"),
            ScanError::NoRoot       => write!(f, "no /Root in trailer"),
            ScanError::MalformedXref => write!(f, "malformed xref table"),
        }
    }
}

impl std::error::Error for ScanError {}

impl From<std::io::Error> for ScanError {
    fn from(e: std::io::Error) -> Self { ScanError::Io(e) }
}

/// Scan a PDF file and return its structural summary.
pub fn scan_structure(path: &Path) -> Result<DocumentStructure, ScanError> {
    let file = std::fs::File::open(path)?;
    // SAFETY: read-only shared mapping; the file is not mutated while the Mmap is live.
    let map = unsafe { memmap2::Mmap::map(&file) }?;
    scan_bytes(&map)
}

/// Scan raw PDF bytes. Exposed for testing without file I/O.
pub(crate) fn scan_bytes(data: &[u8]) -> Result<DocumentStructure, ScanError> {
    let mut leniency = Vec::new();

    if !data.starts_with(b"%PDF-") {
        leniency.push(LeniencyEvent::new("missing-pdf-header", "no %PDF- marker at byte 0"));
    }

    let xref_offset = find_startxref(data).ok_or(ScanError::NoStartxref)?;
    let xref = parse_xref_table(data, xref_offset, &mut leniency)?;
    let trailer = find_trailer(data, xref_offset).ok_or(ScanError::NoTrailer)?;

    let root_ref = find_indirect_ref(trailer, b"/Root").ok_or(ScanError::NoRoot)?;
    let catalog = fetch_object(data, &xref, root_ref.0).unwrap_or(b"");

    let has_acroform = find_key(catalog, b"/AcroForm").is_some();
    let has_xfa = find_key(catalog, b"/XFA").is_some()
        || (has_acroform && fetch_key_dict(data, &xref, catalog, b"/AcroForm")
                .map(|d| find_key(d, b"/XFA").is_some())
                .unwrap_or(false));
    let has_js = find_key(catalog, b"/JS").is_some()
        || names_tree_has_javascript(data, &xref, catalog);

    let page_count = find_indirect_ref(catalog, b"/Pages")
        .and_then(|(n, _)| fetch_object(data, &xref, n))
        .and_then(|obj| parse_int_after_key(obj, b"/Count"))
        .unwrap_or(0) as u32;

    let sig_count = if has_acroform {
        count_sig_field_pattern(
            fetch_key_dict(data, &xref, catalog, b"/AcroForm").unwrap_or(b""),
        )
    } else {
        0
    };

    Ok(DocumentStructure { page_count, has_acroform, has_xfa, has_js, sig_count, leniency })
}

// --- private helpers ---

#[derive(Clone, Default)]
struct XrefEntry { offset: u64, in_use: bool }

/// Scan last 1024 bytes for `startxref\n<N>`, return N as a file offset.
fn find_startxref(data: &[u8]) -> Option<usize> {
    const NEEDLE: &[u8] = b"startxref";
    let search_start = data.len().saturating_sub(1024);
    let tail = &data[search_start..];
    // Find last occurrence of NEEDLE
    let mut last = None;
    for i in 0..=tail.len().saturating_sub(NEEDLE.len()) {
        if &tail[i..i + NEEDLE.len()] == NEEDLE { last = Some(i); }
    }
    let pos = last?;
    let mut i = pos + NEEDLE.len();
    while i < tail.len() && matches!(tail[i], b' ' | b'\r' | b'\n') { i += 1; }
    let start = i;
    while i < tail.len() && tail[i].is_ascii_digit() { i += 1; }
    std::str::from_utf8(&tail[start..i]).ok()?.parse().ok()
}

/// Parse a classic (non-compressed) xref table at `offset`.
fn parse_xref_table(
    data: &[u8],
    offset: usize,
    leniency: &mut Vec<LeniencyEvent>,
) -> Result<Vec<XrefEntry>, ScanError> {
    let d = data.get(offset..).ok_or(ScanError::MalformedXref)?;
    if !d.starts_with(b"xref") {
        // Likely a compressed xref stream (PDF 1.5+) — not supported at M0.
        return Err(ScanError::MalformedXref);
    }
    let mut pos = 4; // after "xref"
    skip_eol(d, &mut pos);

    let mut entries: Vec<XrefEntry> = Vec::new();

    loop {
        // Check for trailer keyword — marks end of xref sections.
        if d.get(pos..).map_or(false, |s| s.starts_with(b"trailer")) { break; }

        let first = match parse_uint(d, &mut pos) { Some(n) => n, None => break };
        skip_ws(d, &mut pos);
        let count = match parse_uint(d, &mut pos) { Some(n) => n, None => break };
        skip_eol(d, &mut pos);

        let needed = first + count;
        if entries.len() < needed { entries.resize(needed, XrefEntry::default()); }

        for obj in first..first + count {
            if pos + 20 > d.len() {
                leniency.push(LeniencyEvent::new("xref-truncated", "xref table ends early"));
                break;
            }
            let entry_bytes = &d[pos..pos + 20];
            // Format: "OOOOOOOOOO GGGGG N/F \n" -- byte 17 is 'n' or 'f'
            let offset_bytes = &entry_bytes[0..10];
            let in_use = entry_bytes.get(17) == Some(&b'n');
            let byte_offset = std::str::from_utf8(offset_bytes)
                .ok()
                .and_then(|s| s.trim().parse::<u64>().ok())
                .unwrap_or(0);
            entries[obj] = XrefEntry { offset: byte_offset, in_use };
            pos += 20;
        }
    }
    Ok(entries)
}

/// Return the slice starting at the trailer dictionary <<...
fn find_trailer<'a>(data: &'a [u8], xref_offset: usize) -> Option<&'a [u8]> {
    let region = data.get(xref_offset..)?;
    let tpos = region.windows(7).position(|w| w == b"trailer")?;
    let after = tpos + 7;
    let dict_start = after + region[after..].iter().position(|&b| b == b'<')?;
    Some(&region[dict_start..])
}

/// Fetch the body of an indirect object by number (between obj/endobj).
fn fetch_object<'a>(data: &'a [u8], xref: &[XrefEntry], num: u32) -> Option<&'a [u8]> {
    let entry = xref.get(num as usize)?;
    if !entry.in_use { return None; }
    let d = data.get(entry.offset as usize..)?;
    // Skip "N G obj" header + whitespace
    let body_start = d.windows(4).position(|w| w == b" obj").map(|p| {
        let after = p + 4;
        after + d[after..].iter().position(|&b| !matches!(b, b' ' | b'\t' | b'\r' | b'\n')).unwrap_or(0)
    })?;
    let body = &d[body_start..];
    let end = body.windows(6).position(|w| w == b"endobj").unwrap_or(body.len());
    Some(&body[..end])
}

/// Find the first occurrence of `key` bytes in `data`. Returns the position of the key start.
fn find_key(data: &[u8], key: &[u8]) -> Option<usize> {
    data.windows(key.len()).position(|w| w == key)
}

/// Find `/Key N G R` in `data` and return (N, G).
fn find_indirect_ref(data: &[u8], key: &[u8]) -> Option<(u32, u16)> {
    let pos = find_key(data, key)?;
    let after = &data[pos + key.len()..];
    let mut i = 0;
    skip_ws(after, &mut i);
    let obj_num = parse_uint(after, &mut i)? as u32;
    skip_ws(after, &mut i);
    let gen_num = parse_uint(after, &mut i)? as u16;
    skip_ws(after, &mut i);
    if after.get(i) == Some(&b'R') { Some((obj_num, gen_num)) } else { None }
}

/// Follow an indirect ref from a named key and return the target object body.
fn fetch_key_dict<'a>(
    data: &'a [u8],
    xref: &[XrefEntry],
    parent: &[u8],
    key: &[u8],
) -> Option<&'a [u8]> {
    let (n, _) = find_indirect_ref(parent, key)?;
    fetch_object(data, xref, n)
}

/// Parse `/Key <integer>` and return the integer value.
fn parse_int_after_key(data: &[u8], key: &[u8]) -> Option<i64> {
    let pos = find_key(data, key)?;
    let after = &data[pos + key.len()..];
    let mut i = 0;
    skip_ws(after, &mut i);
    let neg = after.get(i) == Some(&b'-');
    if neg { i += 1; }
    let start = i;
    while i < after.len() && after[i].is_ascii_digit() { i += 1; }
    if i == start { return None; }
    let n: i64 = std::str::from_utf8(&after[start..i]).ok()?.parse().ok()?;
    Some(if neg { -n } else { n })
}

/// Check if the /Names tree in the catalog has a /JavaScript entry.
fn names_tree_has_javascript(data: &[u8], xref: &[XrefEntry], catalog: &[u8]) -> bool {
    fetch_key_dict(data, xref, catalog, b"/Names")
        .map(|names| find_key(names, b"/JavaScript").is_some())
        .unwrap_or(false)
}

/// Count occurrences of `/FT /Sig` pattern (proxy for sig fields at M0 scope).
fn count_sig_field_pattern(acroform_body: &[u8]) -> u32 {
    // ponytail: pattern match instead of full field-tree walk; sufficient for M0 simple PDFs
    const SIG: &[u8] = b"/FT /Sig";
    let mut count = 0u32;
    let mut i = 0;
    while i + SIG.len() <= acroform_body.len() {
        if &acroform_body[i..i + SIG.len()] == SIG { count += 1; i += SIG.len(); } else { i += 1; }
    }
    count
}

fn parse_uint(data: &[u8], pos: &mut usize) -> Option<usize> {
    let start = *pos;
    while *pos < data.len() && data[*pos].is_ascii_digit() { *pos += 1; }
    if *pos == start { return None; }
    std::str::from_utf8(&data[start..*pos]).ok()?.parse().ok()
}

fn skip_ws(data: &[u8], pos: &mut usize) {
    while *pos < data.len() && matches!(data[*pos], b' ' | b'\t' | b'\r' | b'\n') { *pos += 1; }
}

fn skip_eol(data: &[u8], pos: &mut usize) {
    if data.get(*pos) == Some(&b'\r') { *pos += 1; }
    if data.get(*pos) == Some(&b'\n') { *pos += 1; }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal hand-crafted 1-page PDF with no AcroForm/JS/sigs.
    /// Byte offsets are exact -- do not reformat this literal.
    /// obj1@9  obj2@56  obj3@111  xref@180  startxref=180
    const MINIMAL_PDF: &[u8] = b"\
%PDF-1.0\n\
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

    #[test]
    fn scan_minimal_pdf() {
        let ds = scan_bytes(MINIMAL_PDF).expect("scan should succeed");
        assert_eq!(ds.page_count, 1);
        assert!(!ds.has_acroform);
        assert!(!ds.has_xfa);
        assert!(!ds.has_js);
        assert_eq!(ds.sig_count, 0);
        assert!(ds.leniency.is_empty());
    }
}
