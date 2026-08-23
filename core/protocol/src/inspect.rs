//! Inspect command result type + frame codec. [ADR-025, FR-DIAG-2]
//!
//! Wire codec is a versioned text body for M0 (no bincode yet).

/// Structural summary of a PDF document, returned by the inspect command.
/// Wire type owned by protocol; pdf-cos owns the raw parse result.
#[derive(Debug, Clone, PartialEq)]
pub struct StructuralSummary {
    /// Number of pages reported by the scanner.
    pub page_count: u32,
    /// Document has an AcroForm dictionary.
    pub has_acroform: bool,
    /// Document has XFA.
    pub has_xfa: bool,
    /// Document has JavaScript (catalog/names).
    pub has_js: bool,
    /// Signature field count.
    pub sig_count: u32,
    /// Number of leniency events.
    pub leniency_count: u32,
    /// Human-readable leniency event descriptions (M0: strings; M6: structured).
    pub leniency_events: Vec<String>,
    /// Per-page dimensions as (width_pt_bits, height_pt_bits, rotation_degrees).
    /// Stored as u32 bit-patterns of f32 for Eq/Hash compatibility.
    /// Use [`page_dimensions_f`] to decode.
    pub page_dimensions: Vec<(u32, u32, u32)>,
    /// Original xref table: maps object number -> byte offset in the original file.
    /// Populated by the worker during bootstrap parse for incremental save. [ADR-012]
    pub original_offsets: std::collections::HashMap<u32, u32>,
    /// Object number of the document catalog, from the trailer's `/Root`.
    ///
    /// The coordinator needs it to find the page tree and to write a trailer
    /// that points at the right object; both used to assume 1. [FR-SAVE]
    pub root_obj_num: u32,
}

impl StructuralSummary {
    /// Decode page dimensions from bit-pattern storage to (width_pt, height_pt, rotation).
    pub fn page_dimensions_f(&self) -> Vec<(f32, f32, u32)> {
        self.page_dimensions
            .iter()
            .map(|(w, h, r)| (f32::from_bits(*w), f32::from_bits(*h), *r))
            .collect()
    }

    /// Encode page dimensions from (width_pt, height_pt, rotation) to bit-pattern storage.
    pub fn encode_page_dimensions(dims: &[(f32, f32, u32)]) -> Vec<(u32, u32, u32)> {
        dims.iter()
            .map(|(w, h, r)| (w.to_bits(), h.to_bits(), *r))
            .collect()
    }
}

/// Error decoding a summary frame body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SummaryDecodeError {
    /// Body is not valid UTF-8.
    InvalidUtf8,
    /// Missing or unsupported version.
    BadVersion,
    /// Required field missing or unparsable.
    BadField(&'static str),
}

impl std::fmt::Display for SummaryDecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SummaryDecodeError::InvalidUtf8 => write!(f, "summary is not utf-8"),
            SummaryDecodeError::BadVersion => write!(f, "bad or missing summary version"),
            SummaryDecodeError::BadField(k) => write!(f, "bad or missing field: {k}"),
        }
    }
}

impl std::error::Error for SummaryDecodeError {}

/// Encode a summary as a control-frame body (`SUMMARY` / `v1` text).
pub fn encode_summary(s: &StructuralSummary) -> Vec<u8> {
    let mut out = String::from("SUMMARY\nv1\n");
    out.push_str(&format!("root={}\n", s.root_obj_num));
    out.push_str(&format!("page_count={}\n", s.page_count));
    out.push_str(&format!("has_acroform={}\n", u8::from(s.has_acroform)));
    out.push_str(&format!("has_xfa={}\n", u8::from(s.has_xfa)));
    out.push_str(&format!("has_js={}\n", u8::from(s.has_js)));
    out.push_str(&format!("sig_count={}\n", s.sig_count));
    out.push_str(&format!("leniency_count={}\n", s.leniency_count));
    for e in &s.leniency_events {
        // Newlines in events would break the line protocol; flatten.
        let flat = e.replace(['\n', '\r'], " ");
        out.push_str("leniency=");
        out.push_str(&flat);
        out.push('\n');
    }
    for (w, h, r) in &s.page_dimensions {
        out.push_str(&format!("page_dim={},{},{}\n", f32::from_bits(*w), f32::from_bits(*h), r));
    }
    // Write xref offsets as obj_num:offset pairs.
    let mut sorted_offsets: Vec<_> = s.original_offsets.iter().collect();
    sorted_offsets.sort_by_key(|(k, _)| **k);
    for (obj_num, offset) in sorted_offsets {
        out.push_str(&format!("xref_off={obj_num}:{offset}\n"));
    }
    out.into_bytes()
}

/// Decode a summary frame body produced by [`encode_summary`].
pub fn decode_summary(body: &[u8]) -> Result<StructuralSummary, SummaryDecodeError> {
    let text = std::str::from_utf8(body).map_err(|_| SummaryDecodeError::InvalidUtf8)?;
    let mut lines = text.lines();
    match lines.next() {
        Some("SUMMARY") => {}
        _ => return Err(SummaryDecodeError::BadVersion),
    }
    match lines.next() {
        Some("v1") => {}
        _ => return Err(SummaryDecodeError::BadVersion),
    }

    let mut page_count = None;
    let mut has_acroform = None;
    let mut has_xfa = None;
    let mut has_js = None;
    let mut sig_count = None;
    let mut leniency_count = None;
    let mut leniency_events = Vec::new();
    let mut page_dimensions = Vec::new();
    let mut original_offsets = std::collections::HashMap::new();
    let mut root_obj_num: Option<u32> = None;

    for line in lines {
        if line.is_empty() {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        match k {
            "page_count" => {
                page_count = Some(
                    v.parse()
                        .map_err(|_| SummaryDecodeError::BadField("page_count"))?,
                );
            }
            "has_acroform" => {
                has_acroform = Some(parse_bool01(v).ok_or(SummaryDecodeError::BadField("has_acroform"))?);
            }
            "has_xfa" => {
                has_xfa = Some(parse_bool01(v).ok_or(SummaryDecodeError::BadField("has_xfa"))?);
            }
            "has_js" => {
                has_js = Some(parse_bool01(v).ok_or(SummaryDecodeError::BadField("has_js"))?);
            }
            "sig_count" => {
                sig_count = Some(
                    v.parse()
                        .map_err(|_| SummaryDecodeError::BadField("sig_count"))?,
                );
            }
            "leniency_count" => {
                leniency_count = Some(
                    v.parse()
                        .map_err(|_| SummaryDecodeError::BadField("leniency_count"))?,
                );
            }
            "leniency" => leniency_events.push(v.to_string()),
            "page_dim" => {
                let dims: Vec<&str> = v.split(',').collect();
                if dims.len() == 3 {
                    let w: f32 = dims[0].parse().map_err(|_| SummaryDecodeError::BadField("page_dim.w"))?;
                    let h: f32 = dims[1].parse().map_err(|_| SummaryDecodeError::BadField("page_dim.h"))?;
                    let r: u32 = dims[2].parse().map_err(|_| SummaryDecodeError::BadField("page_dim.r"))?;
                    page_dimensions.push((w.to_bits(), h.to_bits(), r));
                }
            }
            "root" => {
                root_obj_num = v.parse().ok();
            }
            "xref_off" => {
                if let Some((obj_s, off_s)) = v.split_once(':') {
                    if let (Ok(obj), Ok(off)) = (obj_s.parse::<u32>(), off_s.parse::<u32>()) {
                        original_offsets.insert(obj, off);
                    }
                }
            }
            _ => {}
        }
    }

    Ok(StructuralSummary {
        // A document whose trailer named no /Root never reaches here: the scan
        // fails first. Defaulting to 1 would put the old assumption back.
        root_obj_num: root_obj_num.ok_or(SummaryDecodeError::BadField("root"))?,
        page_count: page_count.ok_or(SummaryDecodeError::BadField("page_count"))?,
        has_acroform: has_acroform.ok_or(SummaryDecodeError::BadField("has_acroform"))?,
        has_xfa: has_xfa.ok_or(SummaryDecodeError::BadField("has_xfa"))?,
        has_js: has_js.ok_or(SummaryDecodeError::BadField("has_js"))?,
        sig_count: sig_count.ok_or(SummaryDecodeError::BadField("sig_count"))?,
        leniency_count: leniency_count.ok_or(SummaryDecodeError::BadField("leniency_count"))?,
        leniency_events,
        page_dimensions,
        original_offsets,
    })
}

fn parse_bool01(v: &str) -> Option<bool> {
    match v {
        "0" | "false" => Some(false),
        "1" | "true" => Some(true),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_codec_roundtrip() {
        let mut offsets = std::collections::HashMap::new();
        offsets.insert(1, 100);
        offsets.insert(3, 250);
        let s = StructuralSummary {
            page_count: 3,
            has_acroform: true,
            has_xfa: false,
            has_js: true,
            sig_count: 2,
            leniency_count: 1,
            leniency_events: vec!["missing-pdf-header: no %PDF-".into()],
            page_dimensions: vec![(595.0f32.to_bits(), 842.0f32.to_bits(), 0), (612.0f32.to_bits(), 792.0f32.to_bits(), 90)],
            original_offsets: offsets,
        };
        let bytes = encode_summary(&s);
        let back = decode_summary(&bytes).unwrap();
        assert_eq!(back, s);
        // Verify decode helper works.
        let dims = back.page_dimensions_f();
        assert_eq!(dims, vec![(595.0, 842.0, 0), (612.0, 792.0, 90)]);
    }
}
