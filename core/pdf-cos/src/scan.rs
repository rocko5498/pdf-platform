//! Minimal structural scanner. [ADR-006, SDS Â§14 M0, FR-DIAG-2]
//!
//! Reads classic xref tables only. Compressed xref (PDF 1.5+), encryption,
//! and linearized hint streams are deferred to M1.

use crate::leniency::LeniencyEvent;
use std::path::Path;

/// Structural summary of a PDF document produced by the minimal M0 scanner.
#[derive(Debug)]
pub struct DocumentStructure {
    pub page_count: u32,
    pub has_acroform: bool,
    pub has_xfa: bool,
    pub has_js: bool,
    pub sig_count: u32,
    pub leniency: Vec<LeniencyEvent>,
    /// Parsed xref offsets: maps object number -> byte offset in the file.
    /// Used by IncrementalWriter for correct xref entries during incremental save. [ADR-012]
    pub xref_offsets: std::collections::HashMap<u32, u32>,
    /// Object number of the document catalog, from the trailer's `/Root`.
    ///
    /// Nothing may assume this is 1. Plenty of producers number the catalog
    /// last, and the incremental writer used to hard-code `/Root 1 0 R` into
    /// every trailer it wrote. [FR-SAVE, SDS §3.3]
    pub root_obj_num: u32,
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
            ScanError::Io(e) => write!(f, "I/O error: {e}"),
            ScanError::NoStartxref => write!(f, "no startxref marker found"),
            ScanError::NoTrailer => write!(f, "no trailer dictionary found"),
            ScanError::NoRoot => write!(f, "no /Root in trailer"),
            ScanError::MalformedXref => write!(f, "malformed xref table"),
        }
    }
}

impl std::error::Error for ScanError {}

impl From<std::io::Error> for ScanError {
    fn from(e: std::io::Error) -> Self {
        ScanError::Io(e)
    }
}

/// Scan a PDF file and return its structural summary.
pub fn scan_structure(path: &Path) -> Result<DocumentStructure, ScanError> {
    let file = std::fs::File::open(path)?;
    scan_file(&file)
}

/// Scan an already-opened file (read-only mmap). [ADR-011, SDS Â§3.1 step 4]
pub fn scan_file(file: &std::fs::File) -> Result<DocumentStructure, ScanError> {
    // SAFETY: read-only shared mapping; the file is not mutated while the Mmap is live.
    let map = unsafe { memmap2::Mmap::map(file) }?;
    scan_bytes(&map)
}

/// Scan raw PDF bytes.
///
/// Public so the corrupt-file corpus and randomised sweeps can drive the parser
/// directly. ADR-022 T-4 requires code reachable by untrusted document bytes to
/// be fuzz-targeted, and routing every case through a temp file makes a sweep of
/// any useful size impractical. This is the same code path `scan_file` runs
/// after mmapping, so nothing is bypassed. [ADR-022, T-4, SDS §12.6]
pub fn scan_bytes(data: &[u8]) -> Result<DocumentStructure, ScanError> {
    let mut leniency = Vec::new();

    if !data.starts_with(b"%PDF-") {
        leniency.push(LeniencyEvent::new(
            "missing-pdf-header",
            "no %PDF- marker at byte 0",
        ));
    }

    let xref_offset = find_startxref(data).ok_or(ScanError::NoStartxref)?;
    // The whole chain, not just the newest section: see `parse_xref_chain`.
    let xref = parse_xref_chain(data, xref_offset, &mut leniency)?;
    // Objects a PDF 1.5+ producer stored inside object streams: on such a
    // document the catalog and the page tree are among them, so without this
    // the summary reports a document with no pages. [FR-VIEW-2]
    let inflated = InflatedObjects::decode(data, &xref, &mut leniency);
    // A `startxref` pointing nowhere has no trailer at that offset, and the
    // whole point of reconstruction is to open the file anyway: fall back to
    // the last trailer in the file, then to the catalog itself.
    // [SDS §10.4, FR-VIEW-2]
    let trailer = match section_dictionary(data, xref_offset) {
        Some(trailer) => trailer,
        None => match last_trailer(data) {
            Some(trailer) => {
                leniency.push(LeniencyEvent::new(
                    "trailer-recovered",
                    "no trailer at startxref; used the last trailer in the file",
                ));
                trailer
            }
            None => return Err(ScanError::NoTrailer),
        },
    };

    let root_ref = match find_indirect_ref(&trailer, b"/Root") {
        Some(root) => root,
        None => match find_catalog_object(data, &xref) {
            Some(num) => {
                leniency.push(LeniencyEvent::new(
                    "root-recovered",
                    "trailer names no /Root; used the object typed /Catalog",
                ));
                (num, 0)
            }
            None => return Err(ScanError::NoRoot),
        },
    };
    let catalog = fetch_object(data, &inflated, &xref, root_ref.0).unwrap_or(b"");

    let has_acroform = find_key(catalog, b"/AcroForm").is_some();
    let has_xfa = find_key(catalog, b"/XFA").is_some()
        || (has_acroform
            && fetch_key_dict(data, &inflated, &xref, catalog, b"/AcroForm")
                .map(|d| find_key(d, b"/XFA").is_some())
                .unwrap_or(false));
    let has_js =
        find_key(catalog, b"/JS").is_some() || names_tree_has_javascript(data, &inflated, &xref, catalog);

    let page_count = find_indirect_ref(catalog, b"/Pages")
        .and_then(|(n, _)| fetch_object(data, &inflated, &xref, n))
        .and_then(|obj| parse_int_after_key(obj, b"/Count"))
        .unwrap_or(0) as u32;

    let sig_count = if has_acroform {
        count_sig_field_pattern(fetch_key_dict(data, &inflated, &xref, catalog, b"/AcroForm").unwrap_or(b""))
    } else {
        0
    };

    // Extract xref offsets for incremental save.
    let xref_offsets: std::collections::HashMap<u32, u32> = xref
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.in_use)
        .map(|(obj_num, entry)| (obj_num as u32, entry.offset as u32))
        .collect();

    Ok(DocumentStructure {
        root_obj_num: root_ref.0,
        page_count,
        has_acroform,
        has_xfa,
        has_js,
        sig_count,
        leniency,
        xref_offsets,
    })
}

// --- private helpers ---

/// One cross-reference entry: where an object lives, and whether it is live.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct XrefEntry {
    /// Byte offset of the object in the file. Zero for a compressed object,
    /// which is not at any file offset at all.
    pub offset: u64,
    /// Whether the entry is in use (`n`) rather than free (`f`).
    pub in_use: bool,
    /// For a PDF 1.5+ compressed object: `(object stream number, index within
    /// it)`. Most objects in a modern PDF are stored this way.
    pub compressed: Option<(u32, u32)>,
}

/// Scan last 1024 bytes for `startxref\n<N>`, return N as a file offset.
/// Byte offset recorded by the file's last `startxref`, if any.
///
/// Public because an incremental writer cannot honour `/Prev` without it: the
/// new xref section must point at the one it supersedes. [SDS §3.3, FR-SAVE]
pub fn find_startxref(data: &[u8]) -> Option<usize> {
    const NEEDLE: &[u8] = b"startxref";
    let search_start = data.len().saturating_sub(1024);
    let tail = &data[search_start..];
    // Find the last occurrence of NEEDLE.
    //
    // This was a manual loop over `0..=tail.len().saturating_sub(NEEDLE.len())`.
    // The saturating_sub was there to avoid an underflow, but the range is
    // inclusive, so for any input shorter than the needle it still ran once
    // with i = 0 and sliced `tail[0..9]` out of a shorter slice — a panic on a
    // document of fewer than nine bytes. `scan_file` parses untrusted document
    // bytes inside the Z1 worker, so that panic aborts the worker and the
    // coordinator can only report it as "transport disconnected".
    //
    // `windows` yields nothing when the slice is shorter than the window, which
    // is the behaviour the guard was reaching for.
    // [PRIN-1, T-4, GR-1, GR-8]
    let pos = tail.windows(NEEDLE.len()).rposition(|w| w == NEEDLE)?;
    let mut i = pos + NEEDLE.len();
    while i < tail.len() && matches!(tail[i], b' ' | b'\r' | b'\n') {
        i += 1;
    }
    let start = i;
    while i < tail.len() && tail[i].is_ascii_digit() {
        i += 1;
    }
    std::str::from_utf8(&tail[start..i]).ok()?.parse().ok()
}

/// Parse a classic (non-compressed) xref table at `offset`.
/// Parse one cross-reference section, classic table or PDF 1.5+ stream.
///
/// A cross-reference *stream* is an ordinary object with `/Type /XRef` whose
/// compressed contents hold the same table. `parse_xref_table` rejected it with
/// "not supported at M0", so every COS read on a modern PDF — page objects for
/// stamping, AcroForm fields, optional content — failed on documents most
/// producers have written since 2003. [FR-VIEW-2, SDS §3.1]
fn parse_section(
    data: &[u8],
    offset: usize,
    leniency: &mut Vec<LeniencyEvent>,
) -> Result<Vec<XrefEntry>, ScanError> {
    if data.get(offset..).is_some_and(|d| d.starts_with(b"xref")) {
        return parse_xref_table(data, offset, leniency);
    }

    let body = object_body_at(data, offset).ok_or(ScanError::MalformedXref)?;
    let table = crate::xref::parse_xref_stream(data, body, leniency).map_err(|message| {
        leniency.push(LeniencyEvent::new("xref-stream-unreadable", &message));
        ScanError::MalformedXref
    })?;

    Ok(table
        .entries
        .iter()
        .map(|entry| match entry.entry_type {
            2 => XrefEntry {
                offset: 0,
                in_use: true,
                compressed: Some((entry.obj_stream_num, entry.obj_stream_index)),
            },
            _ => XrefEntry {
                offset: entry.offset,
                in_use: entry.in_use,
                compressed: None,
            },
        })
        .collect())
}

/// The dictionary carrying a section's `/Prev` and `/Root`: the trailer for a
/// classic table, the stream's own dictionary for an xref stream.
///
/// A document written entirely with xref streams has **no `trailer` keyword at
/// all** — `/Root` lives in the stream dictionary — so a reader that insists on
/// finding one rejects every modern PDF. [FR-VIEW-2, SDS §3.1]
pub fn section_dictionary(data: &[u8], offset: usize) -> Option<Vec<u8>> {
    if data.get(offset..).is_some_and(|d| d.starts_with(b"xref")) {
        return find_trailer(data, offset).map(<[u8]>::to_vec);
    }
    object_body_at(data, offset).map(<[u8]>::to_vec)
}

/// The bytes of the object beginning at `offset`, up to and including `endobj`.
fn object_body_at(data: &[u8], offset: usize) -> Option<&[u8]> {
    let rest = data.get(offset..)?;
    const END: &[u8] = b"endobj";
    let end = rest
        .windows(END.len())
        .position(|window| window == END)
        .map(|at| at + END.len())
        .unwrap_or(rest.len());
    Some(&rest[..end])
}

/// The last `trailer` dictionary in the file, wherever it is.
fn last_trailer(data: &[u8]) -> Option<Vec<u8>> {
    const TRAILER: &[u8] = b"trailer";
    let at = data
        .windows(TRAILER.len())
        .rposition(|window| window == TRAILER)?;
    let rest = &data[at..];
    let start = rest.windows(2).position(|w| w == b"<<")?;
    let end = rest[start..].windows(2).rposition(|w| w == b">>")? + start + 2;
    Some(rest[start..end].to_vec())
}

/// The number of the object typed `/Catalog`, for a file whose trailer is gone.
fn find_catalog_object(data: &[u8], xref: &[XrefEntry]) -> Option<u32> {
    let empty = InflatedObjects::default();
    (1..xref.len() as u32).find(|num| {
        fetch_object(data, &empty, xref, *num)
            .map(|object| {
                find_key(object, b"/Type /Catalog").is_some()
                    || (find_key(object, b"/Catalog").is_some()
                        && find_key(object, b"/Pages").is_some())
            })
            .unwrap_or(false)
    })
}

/// Bodies of the objects that live inside object streams, decoded once.
///
/// A PDF 1.5+ producer puts the catalog, the page tree and the AcroForm inside
/// compressed object streams. `fetch_object` returns a slice of the file, so it
/// cannot reach them: the summary reported a document with no pages and no
/// form, which is worse than an error because it looks like an answer. This
/// holds the decompressed bodies so a slice can be returned for those too.
/// [FR-VIEW-2, SDS §3.1, GR-8]
#[derive(Default)]
pub struct InflatedObjects {
    bodies: std::collections::HashMap<u32, Vec<u8>>,
}

/// How much decompressed object-stream content one document may produce.
///
/// Object streams are attacker-controlled input; a small file can declare a
/// very large one. [GR-7]
const MAX_INFLATED_BYTES: usize = 64 * 1024 * 1024;

impl InflatedObjects {
    /// Decode every object stream the cross-reference table refers to.
    #[must_use]
    pub fn decode(data: &[u8], xref: &[XrefEntry], leniency: &mut Vec<LeniencyEvent>) -> Self {
        let mut bodies = std::collections::HashMap::new();
        let mut total = 0usize;

        // Which container holds which object numbers.
        let mut containers: std::collections::HashMap<u32, Vec<(u32, u32)>> =
            std::collections::HashMap::new();
        for (num, entry) in xref.iter().enumerate() {
            if let Some((container, index)) = entry.compressed {
                containers
                    .entry(container)
                    .or_default()
                    .push((num as u32, index));
            }
        }

        for (container, mut members) in containers {
            let Some(entry) = xref.get(container as usize) else {
                continue;
            };
            if entry.compressed.is_some() {
                continue;
            }
            let Some(body) = usize::try_from(entry.offset)
                .ok()
                .and_then(|offset| object_body_at(data, offset))
            else {
                continue;
            };
            let decoded = match crate::xref::decode_object_stream(body) {
                Ok(decoded) => decoded,
                Err(message) => {
                    leniency.push(LeniencyEvent::new("object-stream-unreadable", &message));
                    continue;
                }
            };
            if total.saturating_add(decoded.bytes.len()) > MAX_INFLATED_BYTES {
                leniency.push(LeniencyEvent::new(
                    "object-streams-too-large",
                    "decompressed object streams exceed the per-document limit",
                ));
                break;
            }
            total += decoded.bytes.len();

            let (offsets, first) = &decoded.index;
            members.sort_by_key(|(_, index)| *index);
            for (num, index) in members {
                let Some((_, relative)) = offsets.get(index as usize).copied() else {
                    continue;
                };
                let Some(start) = first.checked_add(relative) else {
                    continue;
                };
                let end = offsets
                    .get(index as usize + 1)
                    .and_then(|(_, next)| first.checked_add(*next))
                    .unwrap_or(decoded.bytes.len())
                    .min(decoded.bytes.len());
                if let Some(slice) = decoded.bytes.get(start..end) {
                    bodies.insert(num, slice.to_vec());
                }
            }
        }

        Self { bodies }
    }

    /// The body of a compressed object, if this holds it.
    #[must_use]
    pub fn body(&self, num: u32) -> Option<&[u8]> {
        self.bodies.get(&num).map(Vec::as_slice)
    }

    /// How many objects were recovered from object streams.
    #[must_use]
    pub fn len(&self) -> usize {
        self.bodies.len()
    }

    /// Whether no compressed object was recovered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bodies.is_empty()
    }
}

/// Fetch a whole object — `N G obj … endobj` — following an object stream when
/// it lives in one.
///
/// `fetch_object` returns only the *body*, which is what this crate's own
/// parsing wants. Callers that rewrite an object (the stamp patcher, rotation,
/// the OCR text layer) need the header too, because what they hand back is
/// written to the file verbatim. A compressed object has no header in the
/// stream, so one is synthesised: to everything downstream it then looks like
/// any other object. [FR-VIEW-2, SDS §3.1]
pub fn fetch_object_bytes(data: &[u8], xref: &[XrefEntry], num: u32) -> Option<Vec<u8>> {
    let entry = xref.get(num as usize)?;

    let Some((container, index)) = entry.compressed else {
        if !entry.in_use {
            return None;
        }
        return object_body_at(data, usize::try_from(entry.offset).ok()?).map(<[u8]>::to_vec);
    };

    let container_entry = xref.get(container as usize)?;
    if container_entry.compressed.is_some() {
        // An object stream inside an object stream is not a thing.
        return None;
    }
    let container_body =
        object_body_at(data, usize::try_from(container_entry.offset).ok()?)?;

    let decoded = crate::xref::decode_object_stream(container_body).ok()?;
    let (offsets, first) = &decoded.index;
    let (_, relative) = offsets.get(index as usize).copied()?;
    let start = first.checked_add(relative)?;
    let end = offsets
        .get(index as usize + 1)
        .and_then(|(_, next)| first.checked_add(*next))
        .unwrap_or(decoded.bytes.len())
        .min(decoded.bytes.len());
    let body = decoded.bytes.get(start..end)?;

    let mut object = format!("{num} 0 obj
").into_bytes();
    object.extend_from_slice(body.strip_suffix(b" ").unwrap_or(body));
    object.extend_from_slice(b"
endobj
");
    Some(object)
}

/// Read the whole cross-reference chain, newest section first. [FR-VIEW-2]
///
/// `parse_xref_table` reads **one** section. An incrementally updated PDF
/// writes a section listing only the objects that changed and points at the
/// previous one with the trailer's `/Prev`; every unchanged object lives in an
/// earlier section. Reading only the newest section therefore resolves nothing
/// but the last edit — which is how most third-party PDFs (anything signed,
/// commented or filled elsewhere) reach us. This product's own writer happens
/// to emit a complete table each time, which is why nothing noticed.
///
/// Entries from newer sections win. A `/Prev` that points forward, repeats an
/// offset already visited, or chains further than `MAX_XREF_SECTIONS` stops the
/// walk: a malformed file must not spin. [GR-7, GR-8]
pub fn parse_xref_chain(data: &[u8], start: usize, leniency: &mut Vec<LeniencyEvent>)
    -> Result<Vec<XrefEntry>, ScanError>
{
    /// A document with more updates than this is either pathological or
    /// hostile; both are answered the same way.
    const MAX_XREF_SECTIONS: usize = 64;

    let mut merged: Vec<XrefEntry> = Vec::new();
    let mut offset = Some(start);
    let mut visited: Vec<usize> = Vec::new();

    while let Some(current) = offset {
        if visited.contains(&current) {
            leniency.push(LeniencyEvent::new(
                "xref-prev-loop",
                "/Prev chain revisits a section; stopping",
            ));
            break;
        }
        if visited.len() >= MAX_XREF_SECTIONS {
            leniency.push(LeniencyEvent::new(
                "xref-prev-too-long",
                "/Prev chain exceeds the section limit; stopping",
            ));
            break;
        }
        visited.push(current);

        let section = match parse_section(data, current, leniency) {
            Ok(section) => section,
            Err(error) => {
                // The newest section failing is fatal; an older one failing
                // leaves us with what we already merged, which is strictly
                // better than nothing and is recorded.
                if visited.len() == 1 {
                    // SDS §10.4 puts qpdf-style reconstruction here: when the
                    // newest section cannot be read, scan the file for object
                    // definitions instead of refusing to open the document.
                    // `reconstruct_xref` has existed since M0 with no caller,
                    // so a damaged startxref failed with "malformed xref table"
                    // and the `xref-reconstructed` event could never fire.
                    // [SDS §10.4, ADR-006, FR-VIEW-2]
                    let rebuilt = crate::xref::reconstruct_xref(data, leniency);
                    if rebuilt.entries.len() > 1 {
                        return Ok(rebuilt
                            .entries
                            .iter()
                            .map(|entry| XrefEntry {
                                offset: entry.offset,
                                in_use: entry.in_use,
                                compressed: None,
                            })
                            .collect());
                    }
                    return Err(error);
                }
                leniency.push(LeniencyEvent::new(
                    "xref-prev-unreadable",
                    "an earlier xref section could not be parsed",
                ));
                break;
            }
        };

        // Newer sections were merged first, so only fill gaps.
        if merged.len() < section.len() {
            merged.resize(section.len(), XrefEntry::default());
        }
        for (index, entry) in section.iter().enumerate() {
            let known = merged[index].offset != 0 || merged[index].in_use;
            if !known && (entry.offset != 0 || entry.in_use) {
                merged[index] = *entry;
            }
        }

        offset = match section_dictionary(data, current)
            .and_then(|dict| parse_int_after_key(&dict, b"/Prev"))
            .and_then(|prev| usize::try_from(prev).ok())
        {
            None => None,
            // A /Prev must point backwards, into the file. Anything else is
            // damage or malice; either way it is reported, not followed.
            Some(prev) if prev < current && prev < data.len() => Some(prev),
            Some(prev) => {
                leniency.push(LeniencyEvent::new(
                    "xref-prev-invalid",
                    &format!("/Prev {prev} does not point backwards into the file"),
                ));
                None
            }
        };
    }

    Ok(merged)
}

pub(crate) fn parse_xref_table(
    data: &[u8],
    offset: usize,
    leniency: &mut Vec<LeniencyEvent>,
) -> Result<Vec<XrefEntry>, ScanError> {
    let d = data.get(offset..).ok_or(ScanError::MalformedXref)?;
    if !d.starts_with(b"xref") {
        // Likely a compressed xref stream (PDF 1.5+) â€” not supported at M0.
        return Err(ScanError::MalformedXref);
    }
    let mut pos = 4; // after "xref"
    skip_eol(d, &mut pos);

    let mut entries: Vec<XrefEntry> = Vec::new();

    loop {
        // Check for trailer keyword â€” marks end of xref sections.
        if d.get(pos..).map_or(false, |s| s.starts_with(b"trailer")) {
            break;
        }

        let first = match parse_uint(d, &mut pos) {
            Some(n) => n,
            None => break,
        };
        skip_ws(d, &mut pos);
        let count = match parse_uint(d, &mut pos) {
            Some(n) => n,
            None => break,
        };
        skip_eol(d, &mut pos);

        let needed = first + count;
        if entries.len() < needed {
            entries.resize(needed, XrefEntry::default());
        }

        for obj in first..first + count {
            if pos + 20 > d.len() {
                leniency.push(LeniencyEvent::new(
                    "xref-truncated",
                    "xref table ends early",
                ));
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
            entries[obj] = XrefEntry {
                offset: byte_offset,
                in_use,
                compressed: None,
            };
            pos += 20;
        }
    }
    Ok(entries)
}

/// Return the slice starting at the trailer dictionary <<...
pub(crate) fn find_trailer<'a>(data: &'a [u8], xref_offset: usize) -> Option<&'a [u8]> {
    let region = data.get(xref_offset..)?;
    let tpos = region.windows(7).position(|w| w == b"trailer")?;
    let after = tpos + 7;
    let dict_start = after + region[after..].iter().position(|&b| b == b'<')?;
    Some(&region[dict_start..])
}

/// Fetch the body of an indirect object by number (between obj/endobj).
pub(crate) fn fetch_object<'a>(
    data: &'a [u8],
    inflated: &'a InflatedObjects,
    xref: &[XrefEntry],
    num: u32,
) -> Option<&'a [u8]> {
    let entry = xref.get(num as usize)?;
    if entry.compressed.is_some() {
        return inflated.body(num);
    }
    if !entry.in_use {
        return None;
    }
    let d = data.get(entry.offset as usize..)?;
    // Skip "N G obj" header + whitespace
    let body_start = d.windows(4).position(|w| w == b" obj").map(|p| {
        let after = p + 4;
        after
            + d[after..]
                .iter()
                .position(|&b| !matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
                .unwrap_or(0)
    })?;
    let body = &d[body_start..];
    let end = body
        .windows(6)
        .position(|w| w == b"endobj")
        .unwrap_or(body.len());
    Some(&body[..end])
}

/// Find the first occurrence of `key` bytes in `data`. Returns the position of the key start.
pub(crate) fn find_key(data: &[u8], key: &[u8]) -> Option<usize> {
    data.windows(key.len()).position(|w| w == key)
}

/// Find `/Key N G R` in `data` and return (N, G).
pub(crate) fn find_indirect_ref(data: &[u8], key: &[u8]) -> Option<(u32, u16)> {
    let pos = find_key(data, key)?;
    let after = &data[pos + key.len()..];
    let mut i = 0;
    skip_ws(after, &mut i);
    let obj_num = parse_uint(after, &mut i)? as u32;
    skip_ws(after, &mut i);
    let gen_num = parse_uint(after, &mut i)? as u16;
    skip_ws(after, &mut i);
    if after.get(i) == Some(&b'R') {
        Some((obj_num, gen_num))
    } else {
        None
    }
}

/// Follow an indirect ref from a named key and return the target object body.
pub(crate) fn fetch_key_dict<'a>(
    data: &'a [u8],
    inflated: &'a InflatedObjects,
    xref: &[XrefEntry],
    parent: &'a [u8],
    key: &[u8],
) -> Option<&'a [u8]> {
    if let Some((n, _)) = find_indirect_ref(parent, key) {
        return fetch_object(data, inflated, xref, n);
    }
    // The value may be a direct dictionary. PDF 32000-1 allows it wherever a
    // dictionary is expected — `/AcroForm << /Fields [...] >>` written inline
    // is a conforming document — and resolving only indirect references meant
    // such a file reported "AcroForm present but not fetchable", imported zero
    // fields, and left the caller to invent some. [FR-FORM-1, PRIN-1]
    inline_dict_after_key(parent, key)
}

/// The `<< ... >>` slice that follows `key`, if the value is a direct dictionary.
pub(crate) fn inline_dict_after_key<'a>(parent: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    let pos = find_key(parent, key)?;
    let mut i = pos + key.len();
    skip_ws(parent, &mut i);
    if parent.get(i) != Some(&b'<') || parent.get(i + 1) != Some(&b'<') {
        return None;
    }
    let start = i;
    let mut depth = 0usize;
    while i + 1 < parent.len() {
        if parent[i] == b'<' && parent[i + 1] == b'<' {
            depth += 1;
            i += 2;
            continue;
        }
        if parent[i] == b'>' && parent[i + 1] == b'>' {
            depth -= 1;
            i += 2;
            if depth == 0 {
                return Some(&parent[start..i]);
            }
            continue;
        }
        i += 1;
    }
    // Unbalanced: refuse rather than hand back a slice that runs to the end of
    // the object. [GR-8]
    None
}

/// Parse `/Key <integer>` and return the integer value.
pub(crate) fn parse_int_after_key(data: &[u8], key: &[u8]) -> Option<i64> {
    let pos = find_key(data, key)?;
    let after = &data[pos + key.len()..];
    let mut i = 0;
    skip_ws(after, &mut i);
    let neg = after.get(i) == Some(&b'-');
    if neg {
        i += 1;
    }
    let start = i;
    while i < after.len() && after[i].is_ascii_digit() {
        i += 1;
    }
    if i == start {
        return None;
    }
    let n: i64 = std::str::from_utf8(&after[start..i]).ok()?.parse().ok()?;
    Some(if neg { -n } else { n })
}

/// Check if the /Names tree in the catalog has a /JavaScript entry.
fn names_tree_has_javascript(
    data: &[u8],
    inflated: &InflatedObjects,
    xref: &[XrefEntry],
    catalog: &[u8],
) -> bool {
    fetch_key_dict(data, inflated, xref, catalog, b"/Names")
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
        if &acroform_body[i..i + SIG.len()] == SIG {
            count += 1;
            i += SIG.len();
        } else {
            i += 1;
        }
    }
    count
}

pub(crate) fn parse_uint(data: &[u8], pos: &mut usize) -> Option<usize> {
    let start = *pos;
    while *pos < data.len() && data[*pos].is_ascii_digit() {
        *pos += 1;
    }
    if *pos == start {
        return None;
    }
    std::str::from_utf8(&data[start..*pos]).ok()?.parse().ok()
}

pub(crate) fn skip_ws(data: &[u8], pos: &mut usize) {
    while *pos < data.len() && matches!(data[*pos], b' ' | b'\t' | b'\r' | b'\n') {
        *pos += 1;
    }
}

pub(crate) fn skip_eol(data: &[u8], pos: &mut usize) {
    if data.get(*pos) == Some(&b'\r') {
        *pos += 1;
    }
    if data.get(*pos) == Some(&b'\n') {
        *pos += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A base document plus a compact incremental update: the second section
    /// lists only the object it changed, and points back with `/Prev`. This is
    /// what Acrobat and every other editor writes; this product's own writer
    /// emits a complete table each time, which is why nothing noticed that only
    /// the newest section was ever read.
    fn incrementally_updated_document() -> Vec<u8> {
        use std::io::Write as _;
        let mut bytes: Vec<u8> = b"%PDF-1.7
".to_vec();
        let mut offsets = Vec::new();
        let objects: Vec<&[u8]> = vec![
            b"<< /Type /Catalog /Pages 2 0 R >>",
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>",
        ];
        for (index, body) in objects.iter().enumerate() {
            offsets.push(bytes.len());
            write!(bytes, "{} 0 obj
", index + 1).unwrap();
            bytes.extend_from_slice(body);
            bytes.extend_from_slice(b"
endobj
");
        }
        let first_xref = bytes.len();
        write!(bytes, "xref
0 {}
", objects.len() + 1).unwrap();
        bytes.extend_from_slice(b"0000000000 65535 f 
");
        for offset in &offsets {
            writeln!(bytes, "{offset:010} 00000 n ").unwrap();
        }
        write!(
            bytes,
            "trailer
<< /Size {} /Root 1 0 R >>
startxref
{first_xref}
%%EOF
",
            objects.len() + 1
        )
        .unwrap();

        // The update: object 3 only.
        let updated_at = bytes.len();
        bytes.extend_from_slice(
            b"3 0 obj
<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] >>
endobj
",
        );
        let second_xref = bytes.len();
        bytes.extend_from_slice(b"xref
3 1
");
        writeln!(bytes, "{updated_at:010} 00000 n ").unwrap();
        write!(
            bytes,
            "trailer
<< /Size 4 /Root 1 0 R /Prev {first_xref} >>
startxref
{second_xref}
%%EOF
"
        )
        .unwrap();
        bytes
    }

    #[test]
    fn an_object_untouched_by_an_incremental_update_is_still_found() {
        let data = incrementally_updated_document();
        let mut leniency = Vec::new();
        let offset = find_startxref(&data).expect("startxref");
        let xref = parse_xref_chain(&data, offset, &mut leniency).expect("chain");

        let inflated = InflatedObjects::default();
        let catalog = fetch_object(&data, &inflated, &xref, 1)
            .expect("the catalog is in the first section");
        assert!(
            String::from_utf8_lossy(catalog).contains("/Type /Catalog"),
            "resolved the wrong object: {:?}",
            String::from_utf8_lossy(catalog)
        );
    }

    #[test]
    fn the_newest_section_wins_for_an_object_it_replaces() {
        let data = incrementally_updated_document();
        let mut leniency = Vec::new();
        let offset = find_startxref(&data).expect("startxref");
        let xref = parse_xref_chain(&data, offset, &mut leniency).expect("chain");

        let inflated = InflatedObjects::default();
        let page = fetch_object(&data, &inflated, &xref, 3).expect("page object");
        let text = String::from_utf8_lossy(page);
        assert!(
            text.contains("595 842"),
            "an update must shadow the object it replaces, got {text:?}"
        );
    }

    #[test]
    fn a_prev_pointing_at_itself_does_not_spin() {
        // A hostile or damaged file must not hang the parser. [GR-7, GR-8]
        let mut data = incrementally_updated_document();
        let second_xref = find_startxref(&data).expect("startxref");
        let trailer_at = data
            .windows(7)
            .rposition(|w| w == b"trailer")
            .expect("trailer");
        let replacement = format!("trailer
<< /Size 4 /Root 1 0 R /Prev {second_xref} >>");
        let end = data[trailer_at..]
            .windows(2)
            .position(|w| w == b">>")
            .expect("dict end")
            + trailer_at
            + 2;
        data.splice(trailer_at..end, replacement.bytes());

        let mut leniency = Vec::new();
        let offset = find_startxref(&data).expect("startxref");
        let xref = parse_xref_chain(&data, offset, &mut leniency).expect("chain");

        assert!(!xref.is_empty(), "the newest section must still be read");
        assert!(
            leniency.iter().any(|event| event.kind.contains("xref-prev")),
            "a self-referential /Prev must be recorded, got {leniency:?}"
        );
    }


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
