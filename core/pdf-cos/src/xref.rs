//! Cross-reference table / xref stream parser and reconstruction. [SDS §3]
//!
//! Supports:
//! - Classic xref tables (PDF 1.0-1.4)
//! - Compressed xref streams (PDF 1.5+) with FlateDecode
//! - Xref reconstruction (qpdf-style) for damaged files
//!
//! The xref is the random-access index that maps object numbers to their
//! byte offsets in the file. PDF 1.5+ can store this as a compressed
//! stream object instead of a classic text table.

use crate::leniency::LeniencyEvent;

/// An entry in the cross-reference table.
#[derive(Debug, Clone, Default)]
pub struct XrefEntry {
    /// Byte offset of the object in the file (for type 1 entries).
    pub offset: u64,
    /// Whether this entry is in use (type 1) or free (type 0).
    pub in_use: bool,
    /// Generation number (for type 1 entries).
    pub generation: u16,
    /// Entry type: 0 = free, 1 = in-use (uncompressed), 2 = compressed (in object stream).
    pub entry_type: u8,
    /// For type 2 entries: object number of the containing object stream.
    pub obj_stream_num: u32,
    /// For type 2 entries: index within the object stream.
    pub obj_stream_index: u32,
}

/// A complete xref table covering all objects in the file.
#[derive(Debug, Clone)]
pub struct XrefTable {
    /// Entries indexed by object number (0-based; entry 0 is the free list head).
    pub entries: Vec<XrefEntry>,
    /// The byte offset of the trailer dictionary (for classic xref).
    pub trailer_offset: Option<usize>,
    /// The /Size value from the trailer (total number of objects).
    pub size: u32,
}

impl XrefTable {
    /// Create an empty xref table.
    pub fn new() -> Self {
        Self {
            entries: vec![XrefEntry::default()], // entry 0 = free list head
            trailer_offset: None,
            size: 0,
        }
    }

    /// Number of objects (including the free-list head at index 0).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the table is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.len() <= 1
    }

    /// Get the entry for an object number.
    pub fn get(&self, obj_num: u32) -> Option<&XrefEntry> {
        self.entries.get(obj_num as usize)
    }

    /// Whether an object number is valid (within range).
    pub fn contains(&self, obj_num: u32) -> bool {
        (obj_num as usize) < self.entries.len()
    }

    /// Merge another xref table (from an incremental update) into this one.
    ///
    /// Later xref entries override earlier ones (incremental updates append new xref sections).
    pub fn merge(&mut self, other: &XrefTable) {
        let max_len = self.entries.len().max(other.entries.len());
        self.entries.resize(max_len, XrefEntry::default());
        for (i, entry) in other.entries.iter().enumerate() {
            if entry.offset != 0 || entry.in_use || entry.entry_type != 0 {
                self.entries[i] = entry.clone();
            }
        }
    }
}

impl Default for XrefTable {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse a classic xref table at the given byte offset.
pub fn parse_classic_xref(
    data: &[u8],
    offset: usize,
    leniency: &mut Vec<LeniencyEvent>,
) -> Result<XrefTable, String> {
    let d = data.get(offset..)
        .ok_or_else(|| format!("xref offset {} out of range", offset))?;

    if !d.starts_with(b"xref") {
        return Err("not a classic xref table (no 'xref' marker)".into());
    }

    let mut table = XrefTable::new();
    let mut pos = 4; // after "xref"

    loop {
        // Skip whitespace and check for end-of-table markers.
        skip_ws(d, &mut pos);
        if d.get(pos..).map_or(false, |s| s.starts_with(b"trailer") || s.starts_with(b"startxref")) {
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
        if table.entries.len() < needed {
            table.entries.resize(needed, XrefEntry::default());
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
            let offset_bytes = &entry_bytes[0..10];
            let gen_bytes = &entry_bytes[11..16];
            let in_use = entry_bytes.get(17) == Some(&b'n');

            let byte_offset = std::str::from_utf8(offset_bytes)
                .ok()
                .and_then(|s| s.trim().parse::<u64>().ok())
                .unwrap_or(0);
            let generation = std::str::from_utf8(gen_bytes)
                .ok()
                .and_then(|s| s.trim().parse::<u16>().ok())
                .unwrap_or(0);

            table.entries[obj] = XrefEntry {
                offset: byte_offset,
                in_use,
                generation,
                entry_type: if in_use { 1 } else { 0 },
                obj_stream_num: 0,
                obj_stream_index: 0,
            };
            pos += 20;
        }
    }

    table.size = table.entries.len() as u32;
    Ok(table)
}

/// Parse a compressed xref stream (PDF 1.5+). [SDS §3, FR-VIEW-2]
///
/// The xref stream is a PDF object with `/Type /XRef` containing binary-encoded
/// xref data. The `/W` array specifies field widths; `/Index` specifies ranges.
pub fn parse_xref_stream(
    // The file itself is not needed: an xref stream is self-contained, and the
    // parameter is kept so callers read the same way for both section kinds.
    _data: &[u8],
    stream_obj_body: &[u8],
    leniency: &mut Vec<LeniencyEvent>,
) -> Result<XrefTable, String> {
    // Parse the stream dictionary to get /W, /Size, /Index, /Root.
    let w = find_stream_int_array(stream_obj_body, b"/W")
        .ok_or("xref stream missing /W array")?;
    if w.len() < 3 {
        return Err("/W array must have at least 3 elements".into());
    }

    let size = find_stream_int(stream_obj_body, b"/Size")
        .unwrap_or(0) as u32;

    let index_ranges = find_stream_int_array(stream_obj_body, b"/Index");

    // Find the stream content between "stream\n" and "endstream".
    let stream_content = find_stream_content(stream_obj_body)
        .ok_or("could not find stream content in xref stream")?;

    // Decompress the stream (usually FlateDecode).
    let decoded = decompress_stream(stream_obj_body, stream_content, leniency)?;

    // Parse the decoded binary data according to /W field widths.
    let w0 = w[0] as usize; // type field width
    let w1 = w[1] as usize; // field-1 width (offset for type 1, obj-num for type 2)
    let w2 = w[2] as usize; // field-2 width (generation for type 1, index for type 2)

    if w0 == 0 && w1 == 0 && w2 == 0 {
        // All-zero /W means all entries are free — valid but empty.
        let mut table = XrefTable::new();
        table.size = size;
        return Ok(table);
    }

    // Determine the ranges from /Index or default (0..Size).
    let ranges: Vec<(u32, u32)> = match index_ranges {
        Some(ref arr) if !arr.is_empty() => {
            arr.chunks(2)
                .map(|chunk| (chunk[0] as u32, chunk.get(1).copied().unwrap_or(0) as u32))
                .collect()
        }
        _ => vec![(0, size)],
    };

    let mut table = XrefTable::new();
    table.size = size;

    let mut decoded_pos = 0;
    for &(start, count) in &ranges {
        let needed = (start + count) as usize;
        if table.entries.len() < needed {
            table.entries.resize(needed, XrefEntry::default());
        }

        for i in 0..count {
            let obj_num = start + i;

            // Read fields according to /W widths.
            let field0 = read_field(&decoded, &mut decoded_pos, w0);
            let field1 = read_field(&decoded, &mut decoded_pos, w1);
            let field2 = read_field(&decoded, &mut decoded_pos, w2);

            match field0 {
                0 => {
                    // Free entry.
                    table.entries[obj_num as usize] = XrefEntry {
                        offset: 0,
                        in_use: false,
                        generation: field2 as u16,
                        entry_type: 0,
                        obj_stream_num: 0,
                        obj_stream_index: 0,
                    };
                }
                1 => {
                    // Uncompressed in-use entry: field1 = byte offset.
                    table.entries[obj_num as usize] = XrefEntry {
                        offset: field1,
                        in_use: true,
                        generation: field2 as u16,
                        entry_type: 1,
                        obj_stream_num: 0,
                        obj_stream_index: 0,
                    };
                }
                2 => {
                    // Compressed in an object stream: field1 = obj-stream number, field2 = index.
                    table.entries[obj_num as usize] = XrefEntry {
                        offset: 0, // actual offset determined from the object stream
                        in_use: true,
                        generation: 0,
                        entry_type: 2,
                        obj_stream_num: field1 as u32,
                        obj_stream_index: field2 as u32,
                    };
                }
                _ => {
                    leniency.push(LeniencyEvent::new(
                        "unknown-xref-type",
                        &format!("unknown xref entry type {} for object {}", field0, obj_num),
                    ));
                }
            }
        }
    }

    Ok(table)
}

/// Reconstruct the xref table by scanning the entire file for `N G obj` patterns.
/// [SDS §10.4, FR-VIEW-2]
///
/// An object stream's decompressed contents and its object index.
pub struct DecodedObjectStream {
    /// The decompressed bytes.
    pub bytes: Vec<u8>,
    /// `(object numbers with their relative offsets, /First)`.
    pub index: (Vec<(u32, usize)>, usize),
}

/// Decode a `/Type /ObjStm` object: decompress it and read its header pairs.
///
/// A modern PDF keeps most of its objects inside these, so a reader that
/// cannot open one cannot see the document's catalog, pages or fields.
/// [FR-VIEW-2, SDS §3.1]
pub fn decode_object_stream(stream_obj_body: &[u8]) -> Result<DecodedObjectStream, String> {
    let count = find_stream_int(stream_obj_body, b"/N").ok_or("object stream missing /N")?;
    let first = find_stream_int(stream_obj_body, b"/First").ok_or("object stream missing /First")?;
    let content = find_stream_content(stream_obj_body).ok_or("object stream has no content")?;

    let mut leniency = Vec::new();
    let bytes = decompress_stream(stream_obj_body, content, &mut leniency)?;

    let header_end = usize::try_from(first).map_err(|_| "/First is out of range")?;
    let header = bytes.get(..header_end).ok_or("object stream shorter than /First")?;
    let numbers: Vec<usize> = String::from_utf8_lossy(header)
        .split_whitespace()
        .filter_map(|token| token.parse().ok())
        .collect();

    let wanted = usize::try_from(count).map_err(|_| "/N is out of range")?;
    let mut index = Vec::with_capacity(wanted);
    for pair in numbers.chunks(2).take(wanted) {
        if pair.len() == 2 {
            index.push((pair[0] as u32, pair[1]));
        }
    }
    if index.len() != wanted {
        return Err(format!(
            "object stream declares {wanted} objects but its header lists {}",
            index.len()
        ));
    }

    Ok(DecodedObjectStream {
        bytes,
        index: (index, header_end),
    })
}

/// This is the qpdf-style recovery: when the xref table is damaged or missing,
/// scan the file for all object definitions and build an xref from them.
pub fn reconstruct_xref(data: &[u8], leniency: &mut Vec<LeniencyEvent>) -> XrefTable {
    leniency.push(LeniencyEvent::new(
        "xref-reconstructed",
        "xref table was damaged; reconstructed by scanning file for object definitions",
    ));

    let mut table = XrefTable::new();
    let pattern = b" obj";

    let mut i = 0;
    while i + 4 <= data.len() {
        if &data[i..i + 4] == pattern {
            // Found " obj" — check if it's preceded by "N G" (object header).
            if i >= 3 {
                // Walk backwards to find the start of the line.
                let line_start = data[..i].iter().rposition(|&b| b == b'\n').map_or(0, |p| p + 1);
                let header = &data[line_start..i];

                // Parse "N G" from the header.
                if let Some((obj_num, gen_num)) = parse_obj_header(header) {
                    let offset = line_start as u64;

                    let needed = (obj_num + 1) as usize;
                    if table.entries.len() < needed {
                        table.entries.resize(needed, XrefEntry::default());
                    }

                    table.entries[obj_num as usize] = XrefEntry {
                        offset,
                        in_use: true,
                        generation: gen_num,
                        entry_type: 1,
                        obj_stream_num: 0,
                        obj_stream_index: 0,
                    };
                }
            }
        }
        i += 1;
    }

    table.size = table.entries.len() as u32;
    table
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Parse an object header "N G" from the bytes before " obj".
fn parse_obj_header(header: &[u8]) -> Option<(u32, u16)> {
    let trimmed = header.trim_ascii();
    let parts: Vec<&[u8]> = trimmed.split(|&b| b == b' ').collect();
    if parts.len() >= 2 {
        let obj_num = std::str::from_utf8(parts[0]).ok()?.parse::<u32>().ok()?;
        let gen_num = std::str::from_utf8(parts[1]).ok()?.parse::<u16>().ok()?;
        Some((obj_num, gen_num))
    } else if parts.len() == 1 {
        let obj_num = std::str::from_utf8(parts[0]).ok()?.parse::<u32>().ok()?;
        Some((obj_num, 0))
    } else {
        None
    }
}

/// Read a big-endian integer of `width` bytes from the decoded stream.
fn read_field(data: &[u8], pos: &mut usize, width: usize) -> u64 {
    let mut value: u64 = 0;
    for _ in 0..width {
        let byte = data.get(*pos).copied().unwrap_or(0);
        value = (value << 8) | (byte as u64);
        *pos += 1;
    }
    value
}

/// Find an integer value after a key in a stream dictionary.
fn find_stream_int(body: &[u8], key: &[u8]) -> Option<i64> {
    let pos = body.windows(key.len()).position(|w| w == key)?;
    let after = &body[pos + key.len()..];
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

/// Find an integer array after a key in a stream dictionary (e.g., `/W [1 3 2]`).
fn find_stream_int_array(body: &[u8], key: &[u8]) -> Option<Vec<i64>> {
    let pos = body.windows(key.len()).position(|w| w == key)?;
    let after = &body[pos + key.len()..];
    let mut i = 0;
    skip_ws(after, &mut i);

    if after.get(i) != Some(&b'[') {
        return None;
    }
    i += 1; // skip '['

    let mut values = Vec::new();
    loop {
        skip_ws(after, &mut i);
        if after.get(i) == Some(&b']') || i >= after.len() {
            break;
        }
        if let Some(v) = parse_signed_int(after, &mut i) {
            values.push(v);
        } else {
            break;
        }
    }
    Some(values)
}

/// Parse a signed integer from bytes.
fn parse_signed_int(data: &[u8], pos: &mut usize) -> Option<i64> {
    skip_ws(data, pos);
    let neg = data.get(*pos) == Some(&b'-');
    if neg {
        *pos += 1;
    }
    let start = *pos;
    while *pos < data.len() && data[*pos].is_ascii_digit() {
        *pos += 1;
    }
    if *pos == start {
        return None;
    }
    let n: i64 = std::str::from_utf8(&data[start..*pos]).ok()?.parse().ok()?;
    Some(if neg { -n } else { n })
}

/// Find the stream content between "stream\n" and "endstream" in a stream object body.
/// Decode a stream object's data, refusing to produce more than `max_bytes`.
///
/// `decompress_stream` is unbounded, which is fine for the document's own
/// object and xref streams but not for an attachment, whose size the document
/// chooses. Only FlateDecode and unfiltered streams are handled; a predictor
/// or another filter is refused rather than returned still encoded. [GR-7, GR-8]
///
/// # Errors
/// When the object has no stream, uses an unsupported filter or decode
/// parameters, or decodes to more than `max_bytes`.
pub fn decode_stream_limited(stream_obj_body: &[u8], max_bytes: usize) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let dict = dictionary_part(stream_obj_body);
    let content = find_stream_content(stream_obj_body).ok_or("object has no stream data")?;
    let has = |needle: &[u8]| dict.windows(needle.len()).any(|w| w == needle);
    if has(b"/DecodeParms") || has(b"/Predictor") {
        return Err("stream uses decode parameters this reader does not apply".into());
    }
    if has(b"/FlateDecode") || has(b"/Fl ") {
        let mut out = Vec::new();
        let mut zlib = flate2::read::ZlibDecoder::new(content).take(max_bytes as u64 + 1);
        zlib.read_to_end(&mut out)
            .map_err(|e| format!("FlateDecode failed: {e}"))?;
        if out.len() > max_bytes {
            return Err(format!("decoded data exceeds the {max_bytes}-byte limit"));
        }
        Ok(out)
    } else if has(b"/Filter") {
        Err("stream uses a filter this reader does not apply".into())
    } else {
        let end = content
            .strip_suffix(b"\r\n")
            .or_else(|| content.strip_suffix(b"\n"))
            .unwrap_or(content);
        if end.len() > max_bytes {
            return Err(format!("data exceeds the {max_bytes}-byte limit"));
        }
        Ok(end.to_vec())
    }
}

fn find_stream_content(body: &[u8]) -> Option<&[u8]> {
    // Look for "stream" followed by \n or \r\n.
    let stream_marker = b"stream";
    let pos = body.windows(stream_marker.len()).position(|w| w == stream_marker)?;
    let after = pos + stream_marker.len();

    // Skip the required end-of-line marker after "stream".
    let mut content_start = after;
    if body.get(content_start) == Some(&b'\r') {
        content_start += 1;
    }
    if body.get(content_start) == Some(&b'\n') {
        content_start += 1;
    }

    // Find "endstream".
    let endstream = body[content_start..].windows(9).position(|w| w == b"endstream")?;
    Some(&body[content_start..content_start + endstream])
}

/// Decompress a stream using the filter specified in the stream dictionary.
fn decompress_stream(
    dict: &[u8],
    content: &[u8],
    leniency: &mut Vec<LeniencyEvent>,
) -> Result<Vec<u8>, String> {
    // Check the /Filter entry.
    //
    // These comparisons used to be `windows(7).any(|w| w == b"/Flate")` and
    // `windows(6).any(|w| w == b"/None")` — seven- and six-byte windows against
    // six- and five-byte needles, which can never be equal. Every stream
    // therefore fell through to "unknown filter; using raw data" and was handed
    // to the caller still compressed. Same shape as the `windows(7)`/`endobj`
    // defect in the worker: a length written by hand next to a literal whose
    // length nobody re-checked. [PRIN-1, GR-8]
    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|window| window == needle)
    }

    if contains(dict, b"/FlateDecode") || contains(dict, b"/Fl ") {
        // FlateDecode (zlib/deflate), then any predictor the dictionary names.
        let inflated = flate2_decompress(content).map_err(|e| {
            leniency.push(LeniencyEvent::new(
                "xref-stream-decompress-failed",
                &format!("FlateDecode failed: {e}"),
            ));
            format!("FlateDecode failed: {e}")
        })?;
        apply_predictor(dict, inflated, leniency)
    } else if contains(dict, b"/None") {
        // No compression.
        Ok(content.to_vec())
    } else {
        // Unknown filter — try raw data.
        leniency.push(LeniencyEvent::new(
            "unknown-stream-filter",
            "unknown stream filter; using raw data",
        ));
        Ok(content.to_vec())
    }
}

/// The stream dictionary: everything before the `stream` keyword.
fn dictionary_part(body: &[u8]) -> &[u8] {
    match body.windows(b"stream".len()).position(|w| w == b"stream") {
        Some(end) => &body[..end],
        None => body,
    }
}

/// Undo the predictor a stream's `/DecodeParms` names. [ISO 32000-1 7.4.4.4]
///
/// Nearly every producer that writes a cross-reference stream (qpdf, pdfTeX,
/// Word, Acrobat) filters it with `/Predictor 12`, the PNG "Up" predictor, to
/// make it compress better. This reader ignored predictors, so the entries came
/// out as noise: `Pages: 0`, `unknown xref entry type 15`, and an object stream
/// that could not be located. The tests that said modern PDFs worked used
/// hand-built streams with no predictor. [FR-VIEW-2, GR-8, PRIN-1]
///
/// PNG predictors 10-15 and TIFF predictor 2 (8 bits per component) are
/// handled. Anything else is an error rather than a silent pass-through, because
/// unfiltered garbage is exactly the failure this fixes.
fn apply_predictor(
    dict: &[u8],
    decoded: Vec<u8>,
    leniency: &mut Vec<LeniencyEvent>,
) -> Result<Vec<u8>, String> {
    let dict = dictionary_part(dict);
    let predictor = find_stream_int(dict, b"/Predictor").unwrap_or(1);
    if predictor <= 1 {
        return Ok(decoded);
    }
    let columns = find_stream_int(dict, b"/Columns").unwrap_or(1);
    let colors = find_stream_int(dict, b"/Colors").unwrap_or(1);
    let bits = find_stream_int(dict, b"/BitsPerComponent").unwrap_or(8);
    if !(1..=(1 << 20)).contains(&columns) || !(1..=32).contains(&colors) || !matches!(bits, 1 | 2 | 4 | 8 | 16) {
        return Err(format!(
            "unusable predictor parameters: Columns {columns}, Colors {colors}, BitsPerComponent {bits}"
        ));
    }
    let row_bytes = ((colors * bits * columns + 7) / 8) as usize;
    let bytes_per_pixel = (((colors * bits) + 7) / 8).max(1) as usize;

    match predictor {
        2 => {
            if bits != 8 {
                return Err(format!("TIFF predictor with {bits} bits per component is not supported"));
            }
            let mut out = decoded;
            for row in out.chunks_mut(row_bytes) {
                for i in bytes_per_pixel..row.len() {
                    row[i] = row[i].wrapping_add(row[i - bytes_per_pixel]);
                }
            }
            Ok(out)
        }
        10..=15 => {
            let stride = row_bytes + 1;
            if decoded.len() % stride != 0 {
                leniency.push(LeniencyEvent::new(
                    "predictor-partial-row",
                    "stream data does not end on a predictor row boundary; the last partial row is ignored",
                ));
            }
            let rows = decoded.len() / stride;
            let mut out = Vec::with_capacity(rows * row_bytes);
            let mut previous = vec![0u8; row_bytes];
            for line in decoded.chunks_exact(stride) {
                let filter = line[0];
                let source = &line[1..];
                let mut current = vec![0u8; row_bytes];
                for i in 0..row_bytes {
                    let left = if i >= bytes_per_pixel { current[i - bytes_per_pixel] } else { 0 };
                    let up = previous[i];
                    let up_left = if i >= bytes_per_pixel { previous[i - bytes_per_pixel] } else { 0 };
                    let predicted = match filter {
                        0 => 0,
                        1 => left,
                        2 => up,
                        3 => ((u16::from(left) + u16::from(up)) / 2) as u8,
                        4 => paeth(left, up, up_left),
                        other => return Err(format!("unknown PNG predictor row filter {other}")),
                    };
                    current[i] = source[i].wrapping_add(predicted);
                }
                out.extend_from_slice(&current);
                previous = current;
            }
            Ok(out)
        }
        other => Err(format!("unsupported predictor {other}")),
    }
}

/// The PNG Paeth predictor (RFC 2083 section 6.6).
fn paeth(left: u8, up: u8, up_left: u8) -> u8 {
    let (a, b, c) = (i32::from(left), i32::from(up), i32::from(up_left));
    let p = a + b - c;
    let (pa, pb, pc) = ((p - a).abs(), (p - b).abs(), (p - c).abs());
    if pa <= pb && pa <= pc {
        left
    } else if pb <= pc {
        up
    } else {
        up_left
    }
}

/// Decompress a `/FlateDecode` stream.
///
/// PDF's FlateDecode is a **zlib** stream (RFC 1950), not a bare deflate one
/// (RFC 1951): two header bytes and a trailing checksum. This used a
/// `DeflateDecoder`, so every well-formed xref stream failed to decompress —
/// the mirror image of the missing zlib header that made OCR recognise nothing.
/// It never showed because nothing called this in production.
///
/// The raw-deflate fallback stays for the writers that omit the header, and it
/// is recorded when it fires rather than silently accepted. [SDS §3, GR-8]
fn flate2_decompress(data: &[u8]) -> Result<Vec<u8>, String> {
    use std::io::Read;

    let mut zlib = flate2::read::ZlibDecoder::new(data);
    let mut output = Vec::new();
    match zlib.read_to_end(&mut output) {
        Ok(_) => return Ok(output),
        Err(_) => output.clear(),
    }

    let mut raw = flate2::read::DeflateDecoder::new(data);
    raw.read_to_end(&mut output)
        .map_err(|e| format!("FlateDecode failed as zlib and as raw deflate: {e}"))?;
    Ok(output)
}

fn parse_uint(data: &[u8], pos: &mut usize) -> Option<usize> {
    let start = *pos;
    while *pos < data.len() && data[*pos].is_ascii_digit() {
        *pos += 1;
    }
    if *pos == start {
        return None;
    }
    std::str::from_utf8(&data[start..*pos]).ok()?.parse().ok()
}

fn skip_ws(data: &[u8], pos: &mut usize) {
    while *pos < data.len() && matches!(data[*pos], b' ' | b'\t' | b'\r' | b'\n') {
        *pos += 1;
    }
}

fn skip_eol(data: &[u8], pos: &mut usize) {
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

    /// The forward PNG filter, written from RFC 2083 independently of the
    /// decoder under test: the decoder must invert this, not mirror itself.
    fn png_encode(rows: &[Vec<u8>], filters: &[u8], bytes_per_pixel: usize) -> Vec<u8> {
        let mut out = Vec::new();
        let mut previous = vec![0u8; rows[0].len()];
        for (row, &filter) in rows.iter().zip(filters) {
            out.push(filter);
            for i in 0..row.len() {
                let left = if i >= bytes_per_pixel { row[i - bytes_per_pixel] } else { 0 };
                let up = previous[i];
                let up_left = if i >= bytes_per_pixel { previous[i - bytes_per_pixel] } else { 0 };
                let predicted = match filter {
                    0 => 0,
                    1 => left,
                    2 => up,
                    3 => ((u16::from(left) + u16::from(up)) / 2) as u8,
                    4 => {
                        let p = i32::from(left) + i32::from(up) - i32::from(up_left);
                        let (pa, pb, pc) = (
                            (p - i32::from(left)).abs(),
                            (p - i32::from(up)).abs(),
                            (p - i32::from(up_left)).abs(),
                        );
                        if pa <= pb && pa <= pc { left } else if pb <= pc { up } else { up_left }
                    }
                    _ => unreachable!(),
                };
                out.push(row[i].wrapping_sub(predicted));
            }
            previous = row.clone();
        }
        out
    }

    fn dict(predictor: i32, columns: i32, colors: i32) -> Vec<u8> {
        format!(
            "<< /Type /XRef /Filter /FlateDecode /DecodeParms << /Columns {columns} /Colors {colors} \
             /Predictor {predictor} >> >>\nstream\n"
        )
        .into_bytes()
    }

    #[test]
    fn every_png_filter_type_is_undone() {
        // Four rows, one per filter type, over varied bytes including values
        // that overflow on addition.
        let rows: Vec<Vec<u8>> = vec![
            vec![10, 200, 33, 250, 7, 99],
            vec![11, 190, 40, 255, 0, 120],
            vec![250, 5, 60, 1, 128, 129],
            vec![0, 255, 17, 90, 91, 92],
            vec![200, 100, 50, 25, 12, 6],
        ];
        for bytes_per_pixel in [1usize, 2] {
            let filters = [0u8, 1, 2, 3, 4];
            let encoded = png_encode(&rows, &filters, bytes_per_pixel);
            let columns = (rows[0].len() / bytes_per_pixel) as i32;
            let mut leniency = Vec::new();
            let decoded = apply_predictor(&dict(15, columns, bytes_per_pixel as i32), encoded, &mut leniency)
                .expect("decode");
            let expected: Vec<u8> = rows.concat();
            assert_eq!(decoded, expected, "bytes per pixel {bytes_per_pixel}");
            assert!(leniency.is_empty());
        }
    }

    #[test]
    fn a_stream_without_a_predictor_is_left_alone() {
        let data = vec![1u8, 2, 3, 4];
        let out = apply_predictor(b"<< /Filter /FlateDecode >>\nstream\n", data.clone(), &mut Vec::new()).unwrap();
        assert_eq!(out, data);
        let out = apply_predictor(&dict(1, 4, 1), data.clone(), &mut Vec::new()).unwrap();
        assert_eq!(out, data, "/Predictor 1 means none");
    }

    #[test]
    fn the_tiff_predictor_adds_each_byte_to_the_one_before() {
        // Row of 4 single-byte pixels: 1, +1, +1, +1 -> 1, 2, 3, 4.
        let out = apply_predictor(&dict(2, 4, 1), vec![1, 1, 1, 1], &mut Vec::new()).unwrap();
        assert_eq!(out, [1, 2, 3, 4]);
    }

    #[test]
    fn what_cannot_be_undone_is_an_error_not_noise() {
        // An unknown row filter, a predictor we do not implement, and nonsense
        // parameters must fail loudly: passing the bytes through is the bug.
        assert!(apply_predictor(&dict(12, 4, 1), vec![9, 1, 2, 3, 4], &mut Vec::new()).is_err());
        assert!(apply_predictor(&dict(7, 4, 1), vec![0; 8], &mut Vec::new()).is_err());
        assert!(apply_predictor(&dict(12, 0, 1), vec![0; 8], &mut Vec::new()).is_err());
    }

    #[test]
    fn a_trailing_partial_row_is_dropped_and_recorded() {
        let mut leniency = Vec::new();
        let encoded = png_encode(&[vec![1, 2, 3, 4], vec![5, 6, 7, 8]], &[0, 0], 1);
        let mut data = encoded;
        data.extend_from_slice(&[0, 9]); // a row cut short
        let out = apply_predictor(&dict(12, 4, 1), data, &mut leniency).unwrap();
        assert_eq!(out, [1, 2, 3, 4, 5, 6, 7, 8]);
        assert!(leniency.iter().any(|e| e.kind.contains("predictor-partial-row")), "{leniency:?}");
    }

    #[test]
    fn classic_xref_parse() {
        let xref_data = b"xref\n0 4\n0000000000 65535 f \n0000000009 00000 n \n0000000056 00000 n \n0000000111 00000 n \n";
        let mut leniency = Vec::new();
        let table = parse_classic_xref(xref_data, 0, &mut leniency).unwrap();
        assert!(table.len() >= 4, "table should have at least 4 entries, got {}", table.len());
        assert!(!table.entries[0].in_use);
        assert!(table.entries[1].in_use);
        assert_eq!(table.entries[1].offset, 9);
        assert!(table.entries[2].in_use);
        assert_eq!(table.entries[2].offset, 56);
    }

    #[test]
    fn xref_table_merge() {
        let mut t1 = XrefTable::new();
        t1.entries.resize(3, XrefEntry::default());
        t1.entries[1] = XrefEntry { offset: 100, in_use: true, generation: 0, entry_type: 1, obj_stream_num: 0, obj_stream_index: 0 };

        let mut t2 = XrefTable::new();
        t2.entries.resize(4, XrefEntry::default());
        t2.entries[1] = XrefEntry { offset: 200, in_use: true, generation: 1, entry_type: 1, obj_stream_num: 0, obj_stream_index: 0 };
        t2.entries[3] = XrefEntry { offset: 300, in_use: true, generation: 0, entry_type: 1, obj_stream_num: 0, obj_stream_index: 0 };

        t1.merge(&t2);
        assert_eq!(t1.len(), 4);
        assert_eq!(t1.entries[1].offset, 200); // overridden
        assert_eq!(t1.entries[3].offset, 300); // added
    }

    #[test]
    fn reconstruct_xref_finds_objects() {
        let pdf = b"%PDF-1.0\n1 0 obj\n<< /Type /Catalog >>\nendobj\n2 0 obj\n<< /Type /Pages >>\nendobj\n";
        let mut leniency = Vec::new();
        let table = reconstruct_xref(pdf, &mut leniency);
        assert_eq!(table.len(), 3); // 0 + 1 + 2
        assert!(table.entries[1].in_use);
        assert!(table.entries[2].in_use);
        assert_eq!(table.entries[1].offset, 9); // "1 0 obj" starts at byte 9
    }

    #[test]
    fn parse_obj_header_basic() {
        assert_eq!(parse_obj_header(b"1 0"), Some((1, 0)));
        assert_eq!(parse_obj_header(b"42 3"), Some((42, 3)));
        assert_eq!(parse_obj_header(b"7"), Some((7, 0)));
        assert_eq!(parse_obj_header(b""), None);
    }

    #[test]
    fn read_field_big_endian() {
        let data = [0x00, 0x00, 0x01, 0x00]; // 256 in big-endian 4-byte
        let mut pos = 0;
        assert_eq!(read_field(&data, &mut pos, 4), 256);
        assert_eq!(pos, 4);

        let data2 = [0x01, 0x00]; // 256 in big-endian 2-byte
        let mut pos2 = 0;
        assert_eq!(read_field(&data2, &mut pos2, 2), 256);
    }

    #[test]
    fn find_stream_int_array() {
        let body = b"/W [1 3 2]";
        let arr = super::find_stream_int_array(body, b"/W").unwrap();
        assert_eq!(arr, vec![1, 3, 2]);
    }

    #[test]
    fn find_stream_content_test() {
        // Content is everything between "stream\n" and "endstream".
        // The newline before "endstream" is part of the content.
        let body = b"<< /Length 10 >>\nstream\n0123456789\nendstream";
        let content = super::find_stream_content(body).unwrap();
        assert_eq!(content, b"0123456789\n");
    }
}
