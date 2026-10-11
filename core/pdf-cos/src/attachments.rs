//! Embedded files (attachments) read from the document's own objects.
//! [FR-EMB-1, FR-EMB-3, ADR-005]
//!
//! The engine lists attachments by name and size but offers no way to get the
//! bytes out, so "access" stopped at a label. The name tree and the file
//! streams are ordinary COS objects, read here the way `ocg` reads layers.
//!
//! Embedded files are untrusted data (FR-EMB-4): this module only decodes
//! bytes. It never opens, runs or interprets them.

use crate::scan::{
    fetch_object_bytes, find_indirect_ref, find_key, find_startxref, find_trailer, parse_uint,
    parse_xref_chain, skip_ws,
};
use crate::xref::decode_stream_limited;

/// Name-tree nodes visited before giving up, so a hostile tree cannot loop or
/// fan out without bound. [GR-7]
const MAX_NODES: usize = 2000;
const MAX_DEPTH: usize = 8;

type Fetch<'a> = &'a dyn Fn(u32) -> Option<Vec<u8>>;

/// One embedded file as the document declares it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddedFile {
    /// `/UF` if present, else `/F`, else the name-tree key.
    pub name: String,
    /// Object number of the file's data stream.
    pub stream_obj: u32,
}

/// List the document's embedded files.
///
/// Empty for a document with none, and for one whose cross-reference data
/// cannot be read; the caller cannot tell those apart and does not need to.
#[must_use]
pub fn list_embedded_files(data: &[u8]) -> Vec<EmbeddedFile> {
    let Some(xref_offset) = find_startxref(data) else {
        return Vec::new();
    };
    let mut leniency = Vec::new();
    let Ok(xref) = parse_xref_chain(data, xref_offset, &mut leniency) else {
        return Vec::new();
    };
    let Some(trailer) = find_trailer(data, xref_offset) else {
        return Vec::new();
    };
    let Some((root, _)) = find_indirect_ref(trailer, b"/Root") else {
        return Vec::new();
    };
    let fetch = |num: u32| fetch_object_bytes(data, &xref, num);
    let Some(catalog) = fetch(root) else {
        return Vec::new();
    };

    let Some(names) = dict_value(&catalog, b"/Names", &fetch) else {
        return Vec::new();
    };
    let Some(embedded) = dict_value(&names, b"/EmbeddedFiles", &fetch) else {
        return Vec::new();
    };

    let mut files = Vec::new();
    let mut visited = 0usize;
    walk_name_tree(&embedded, &fetch, 0, &mut visited, &mut files);
    files
}

/// Decode one embedded file's bytes, refusing more than `max_bytes` so a
/// compression bomb cannot exhaust memory. [GR-7, FR-EMB-4]
///
/// # Errors
/// When the stream is missing, uses a filter this reader does not apply, or
/// decodes to more than `max_bytes`.
pub fn extract_embedded_file(
    data: &[u8],
    file: &EmbeddedFile,
    max_bytes: usize,
) -> Result<Vec<u8>, String> {
    let xref_offset = find_startxref(data).ok_or("no startxref")?;
    let mut leniency = Vec::new();
    let xref = parse_xref_chain(data, xref_offset, &mut leniency).map_err(|e| format!("{e:?}"))?;
    let object = fetch_object_bytes(data, &xref, file.stream_obj)
        .ok_or_else(|| format!("object {} not found", file.stream_obj))?;
    decode_stream_limited(&object, max_bytes)
}

fn walk_name_tree(
    node: &[u8],
    fetch: Fetch,
    depth: usize,
    visited: &mut usize,
    out: &mut Vec<EmbeddedFile>,
) {
    *visited += 1;
    if depth > MAX_DEPTH || *visited > MAX_NODES {
        return;
    }
    // Leaf: /Names [ (key) filespec (key) filespec ... ]
    if let Some(array) = array_after(node, b"/Names") {
        let mut i = 0;
        while i < array.len() {
            skip_ws(&array, &mut i);
            let Some(key) = read_string(&array, &mut i) else {
                break;
            };
            skip_ws(&array, &mut i);
            let Some(spec) = read_ref_or_dict(&array, &mut i, fetch) else {
                break;
            };
            if let Some(file) = filespec(&spec, &key, fetch) {
                out.push(file);
            }
        }
    }
    // Intermediate: /Kids [ N G R ... ]
    if let Some(array) = array_after(node, b"/Kids") {
        for num in refs_in(&array) {
            if let Some(child) = fetch(num) {
                walk_name_tree(&child, fetch, depth + 1, visited, out);
            }
        }
    }
}

fn filespec(spec: &[u8], key: &str, fetch: Fetch) -> Option<EmbeddedFile> {
    let ef = dict_value(spec, b"/EF", fetch)?;
    let (stream_obj, _) =
        find_indirect_ref(&ef, b"/UF").or_else(|| find_indirect_ref(&ef, b"/F"))?;
    let name = string_after(spec, b"/UF")
        .or_else(|| string_after(spec, b"/F"))
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| key.to_string());
    Some(EmbeddedFile { name, stream_obj })
}

/// The dictionary a key holds, whether written inline or as a reference.
fn dict_value(parent: &[u8], key: &[u8], fetch: Fetch) -> Option<Vec<u8>> {
    if let Some((num, _)) = find_indirect_ref(parent, key) {
        return fetch(num);
    }
    let pos = find_key(parent, key)? + key.len();
    let rest = &parent[pos..];
    let mut i = 0;
    skip_ws(rest, &mut i);
    balanced_dict(rest, &mut i)
}

/// A balanced `<< >>` starting at `*i`; leaves `*i` after it.
fn balanced_dict(data: &[u8], i: &mut usize) -> Option<Vec<u8>> {
    if data.get(*i..*i + 2) != Some(b"<<") {
        return None;
    }
    let start = *i;
    let mut depth = 0usize;
    while *i + 1 < data.len() {
        if &data[*i..*i + 2] == b"<<" {
            depth += 1;
            *i += 2;
        } else if &data[*i..*i + 2] == b">>" {
            depth -= 1;
            *i += 2;
            if depth == 0 {
                return Some(data[start..*i].to_vec());
            }
        } else {
            *i += 1;
        }
    }
    None
}

/// The `[ ... ]` body after a key.
fn array_after(node: &[u8], key: &[u8]) -> Option<Vec<u8>> {
    let pos = find_key(node, key)? + key.len();
    let rest = &node[pos..];
    let mut i = 0;
    skip_ws(rest, &mut i);
    if rest.get(i) != Some(&b'[') {
        return None;
    }
    // Brackets inside a string are characters, not structure.
    let mut depth = 0usize;
    let mut in_string = 0usize;
    let mut j = i;
    while j < rest.len() {
        match rest[j] {
            b'\\' if in_string > 0 => j += 1,
            b'(' => in_string += 1,
            b')' if in_string > 0 => in_string -= 1,
            b'[' if in_string == 0 => depth += 1,
            b']' if in_string == 0 => {
                depth -= 1;
                if depth == 0 {
                    return Some(rest[i + 1..j].to_vec());
                }
            }
            _ => {}
        }
        j += 1;
    }
    None
}

fn refs_in(array: &[u8]) -> Vec<u32> {
    let text = String::from_utf8_lossy(array);
    let tokens: Vec<&str> = text.split_whitespace().collect();
    tokens
        .windows(3)
        .filter(|w| w[2] == "R")
        .filter_map(|w| w[0].parse().ok())
        .collect()
}

/// A filespec in a name array: `N G R` (fetched) or an inline `<< >>`.
fn read_ref_or_dict(array: &[u8], i: &mut usize, fetch: Fetch) -> Option<Vec<u8>> {
    skip_ws(array, i);
    if array.get(*i..*i + 2) == Some(b"<<") {
        return balanced_dict(array, i);
    }
    let num = parse_uint(array, i)? as u32;
    skip_ws(array, i);
    parse_uint(array, i)?;
    skip_ws(array, i);
    if array.get(*i) != Some(&b'R') {
        return None;
    }
    *i += 1;
    fetch(num)
}

fn string_after(dict: &[u8], key: &[u8]) -> Option<String> {
    let pos = find_key(dict, key)? + key.len();
    let rest = &dict[pos..];
    let mut i = 0;
    skip_ws(rest, &mut i);
    read_string(rest, &mut i)
}

/// A literal `( )` or hex `< >` PDF string, decoded to text (UTF-16BE aware).
fn read_string(data: &[u8], i: &mut usize) -> Option<String> {
    skip_ws(data, i);
    let bytes: Vec<u8> = match data.get(*i)? {
        b'(' => {
            *i += 1;
            let mut out = Vec::new();
            let mut depth = 1usize;
            while *i < data.len() {
                let c = data[*i];
                *i += 1;
                match c {
                    b'\\' => {
                        let e = *data.get(*i)?;
                        *i += 1;
                        match e {
                            b'n' => out.push(b'\n'),
                            b'r' => out.push(b'\r'),
                            b't' => out.push(b'\t'),
                            b'b' => out.push(8),
                            b'f' => out.push(12),
                            b'0'..=b'7' => {
                                let mut v = u32::from(e - b'0');
                                for _ in 0..2 {
                                    match data.get(*i) {
                                        Some(d @ b'0'..=b'7') => {
                                            v = v * 8 + u32::from(d - b'0');
                                            *i += 1;
                                        }
                                        _ => break,
                                    }
                                }
                                out.push((v & 0xFF) as u8);
                            }
                            b'\r' | b'\n' => {}
                            other => out.push(other),
                        }
                    }
                    b'(' => {
                        depth += 1;
                        out.push(c);
                    }
                    b')' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                        out.push(c);
                    }
                    _ => out.push(c),
                }
            }
            out
        }
        b'<' if data.get(*i + 1) != Some(&b'<') => {
            *i += 1;
            let mut digits = Vec::new();
            while *i < data.len() && data[*i] != b'>' {
                if data[*i].is_ascii_hexdigit() {
                    digits.push(data[*i]);
                }
                *i += 1;
            }
            *i += 1;
            if digits.len() % 2 == 1 {
                digits.push(b'0');
            }
            digits
                .chunks(2)
                .filter_map(|p| u8::from_str_radix(std::str::from_utf8(p).ok()?, 16).ok())
                .collect()
        }
        _ => return None,
    };
    Some(decode_text(&bytes))
}

fn decode_text(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|p| u16::from_be_bytes([p[0], p[1]]))
            .collect();
        return String::from_utf16_lossy(&units);
    }
    String::from_utf8_lossy(bytes).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn zlib(data: &[u8]) -> Vec<u8> {
        let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(data).unwrap();
        enc.finish().unwrap()
    }

    /// Catalog -> Names -> EmbeddedFiles -> two filespecs: a plain stream whose
    /// name needs an escape, and a Flate stream named in UTF-16.
    fn pdf_with_attachments(plain: &[u8], packed_source: &[u8]) -> Vec<u8> {
        let packed = zlib(packed_source);
        let mut o8 = format!("<< /Length {} >>\nstream\n", plain.len()).into_bytes();
        o8.extend_from_slice(plain);
        o8.extend_from_slice(b"\nendstream");
        let mut o9 =
            format!("<< /Length {} /Filter /FlateDecode >>\nstream\n", packed.len()).into_bytes();
        o9.extend_from_slice(&packed);
        o9.extend_from_slice(b"\nendstream");
        let objects: Vec<Vec<u8>> = vec![
            b"<< /Type /Catalog /Pages 2 0 R /Names << /EmbeddedFiles 5 0 R >> >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] >>".to_vec(),
            b"<< >>".to_vec(),
            b"<< /Names [ (a\\(1\\).txt) 6 0 R <FEFF0062002E007400780074> 7 0 R ] >>".to_vec(),
            b"<< /Type /Filespec /F (a(1).txt) /EF << /F 8 0 R >> >>".to_vec(),
            b"<< /Type /Filespec /UF <FEFF0062002E007400780074> /EF << /F 9 0 R >> >>".to_vec(),
            o8,
            o9,
        ];

        let mut out = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (i, body) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend(format!("{} 0 obj\n", i + 1).bytes());
            out.extend_from_slice(body);
            out.extend(b"\nendobj\n");
        }
        let xref = out.len();
        out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).bytes());
        for o in offsets {
            out.extend(format!("{o:010} 00000 n \n").bytes());
        }
        out.extend(
            format!(
                "trailer\n<</Size {} /Root 1 0 R>>\nstartxref\n{xref}\n%%EOF",
                objects.len() + 1
            )
            .bytes(),
        );
        out
    }

    #[test]
    fn the_names_and_bytes_of_embedded_files_come_out_intact() {
        let plain = b"hello attachment\nline two";
        let source = b"compressed payload ".repeat(50);
        let pdf = pdf_with_attachments(plain, &source);

        let files = list_embedded_files(&pdf);
        let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["a(1).txt", "b.txt"]);

        assert_eq!(extract_embedded_file(&pdf, &files[0], 1 << 20).unwrap(), plain);
        assert_eq!(extract_embedded_file(&pdf, &files[1], 1 << 20).unwrap(), source);
    }

    #[test]
    fn a_decompression_bomb_is_refused_not_expanded() {
        let source = vec![0u8; 5_000_000];
        let pdf = pdf_with_attachments(b"x", &source);
        let files = list_embedded_files(&pdf);
        let err = extract_embedded_file(&pdf, &files[1], 1_000_000).unwrap_err();
        assert!(err.contains("exceeds"), "{err}");
    }

    #[test]
    fn a_document_without_attachments_lists_none() {
        let bytes = b"%PDF-1.4\n1 0 obj\n<< /Type /Catalog >>\nendobj\ntrailer\n<< /Root 1 0 R >>\nstartxref\n0\n%%EOF";
        assert!(list_embedded_files(bytes).is_empty());
    }
}
