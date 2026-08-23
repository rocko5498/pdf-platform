//! Optional content groups (layers) read from the document's own objects.
//! [FR-VIEW-4, ADR-005]
//!
//! PDFium's public API — as exposed by `pdfium-render` — has no optional
//! content accessor at all, so `PdfiumEngine::layers` returned an empty list
//! for every document and the shell's Layers panel said "none" whether or not
//! the document had any. The information is in the catalog, and this crate
//! already parses the catalog, so it is read here rather than left unanswered.

use crate::scan::{
    fetch_object_bytes, find_indirect_ref, find_key, find_startxref, find_trailer, parse_xref_chain,
};

/// One optional content group as the document declares it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionalContentGroup {
    /// PDF object number of the group, which is its stable identity.
    pub obj_num: u32,
    /// `/Name`, as shown to the user.
    pub name: String,
    /// Whether the default configuration turns it on.
    ///
    /// A group is on unless the default configuration's `/OFF` array names it,
    /// or `/BaseState` is `/OFF` and its `/ON` array does not.
    pub visible: bool,
}

/// Read the document's optional content groups.
///
/// Returns an empty vector for a document that declares none — which is most
/// documents — and for one whose cross-reference table cannot be parsed. The
/// caller cannot tell those apart, and does not need to: a document whose xref
/// is unreadable fails earlier, on open.
#[must_use]
pub fn parse_optional_content_groups(data: &[u8]) -> Vec<OptionalContentGroup> {
    let Some(xref_offset) = find_startxref(data) else {
        return Vec::new();
    };
    // The whole /Prev chain: a document whose last update added a signature
    // keeps its optional content in an earlier section.
    let mut leniency = Vec::new();
    let Ok(xref) = parse_xref_chain(data, xref_offset, &mut leniency) else {
        return Vec::new();
    };
    let Some(trailer) = find_trailer(data, xref_offset) else {
        return Vec::new();
    };
    let Some((root_num, _)) = find_indirect_ref(trailer, b"/Root") else {
        return Vec::new();
    };
    let Some(catalog) = fetch_object_bytes(data, &xref, root_num) else {
        return Vec::new();
    };

    // /OCProperties may be inline in the catalog or an indirect object.
    let properties: Vec<u8> = match find_indirect_ref(&catalog, b"/OCProperties") {
        Some((num, _)) => match fetch_object_bytes(data, &xref, num) {
            Some(object) => object,
            None => return Vec::new(),
        },
        None => match find_key(&catalog, b"/OCProperties") {
            Some(_) => catalog.clone(),
            None => return Vec::new(),
        },
    };

    let ocgs = match array_after_key(&properties, b"/OCGs") {
        Some(list) => list,
        None => return Vec::new(),
    };

    // The default configuration decides what is on. /BaseState defaults to /ON.
    let default_config = dict_after_key(&properties, b"/D").unwrap_or_default();
    let base_on = !contains_token(&default_config, b"/BaseState /OFF")
        && !contains_token(&default_config, b"/BaseState/OFF");
    let off: Vec<u32> = array_after_key(&default_config, b"/OFF").unwrap_or_default();
    let on: Vec<u32> = array_after_key(&default_config, b"/ON").unwrap_or_default();

    ocgs.into_iter()
        .filter_map(|obj_num| {
            let object = fetch_object_bytes(data, &xref, obj_num)?;
            Some(OptionalContentGroup {
                obj_num,
                name: pdf_string_after_key(&object, b"/Name")
                    .unwrap_or_else(|| format!("Layer {obj_num}")),
                visible: if base_on {
                    !off.contains(&obj_num)
                } else {
                    on.contains(&obj_num)
                },
            })
        })
        .collect()
}

/// The object numbers of the indirect references in the array at `key`.
fn array_after_key(data: &[u8], key: &[u8]) -> Option<Vec<u32>> {
    let at = find_key(data, key)?;
    let rest = &data[at + key.len()..];
    let start = rest.iter().position(|b| *b == b'[')?;
    let end = start + rest[start..].iter().position(|b| *b == b']')?;
    let body = String::from_utf8_lossy(&rest[start + 1..end]).to_string();

    let tokens: Vec<&str> = body.split_whitespace().collect();
    let mut refs = Vec::new();
    for window in tokens.windows(3) {
        if window[2] == "R" {
            if let Ok(num) = window[0].parse::<u32>() {
                refs.push(num);
            }
        }
    }
    Some(refs)
}

/// The dictionary that follows `key`, including its `<<`/`>>`.
fn dict_after_key(data: &[u8], key: &[u8]) -> Option<Vec<u8>> {
    let at = find_key(data, key)?;
    let rest = &data[at + key.len()..];
    let start = rest.windows(2).position(|w| w == b"<<")?;
    let mut depth = 0i32;
    let mut index = start;
    while index + 1 < rest.len() {
        if &rest[index..index + 2] == b"<<" {
            depth += 1;
            index += 2;
            continue;
        }
        if &rest[index..index + 2] == b">>" {
            depth -= 1;
            index += 2;
            if depth == 0 {
                return Some(rest[start..index].to_vec());
            }
            continue;
        }
        index += 1;
    }
    None
}

/// A literal `(string)` value following `key`.
fn pdf_string_after_key(data: &[u8], key: &[u8]) -> Option<String> {
    let at = find_key(data, key)?;
    let rest = &data[at + key.len()..];
    let start = rest.iter().position(|b| *b == b'(')?;

    let mut depth = 0i32;
    let mut out = Vec::new();
    let mut index = start;
    while index < rest.len() {
        match rest[index] {
            b'\\' if index + 1 < rest.len() => {
                out.push(rest[index + 1]);
                index += 2;
                continue;
            }
            b'(' => {
                depth += 1;
                if depth > 1 {
                    out.push(b'(');
                }
            }
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(String::from_utf8_lossy(&out).to_string());
                }
                out.push(b')');
            }
            byte => out.push(byte),
        }
        index += 1;
    }
    None
}

/// Whether `needle` appears in `data`, ignoring runs of whitespace.
fn contains_token(data: &[u8], needle: &[u8]) -> bool {
    let text: String = String::from_utf8_lossy(data)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let want: String = String::from_utf8_lossy(needle)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    text.contains(&want)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A document with two optional content groups, the second turned off.
    fn document_with_layers() -> Vec<u8> {
        let objects: Vec<Vec<u8>> = vec![
            b"<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [4 0 R 5 0 R] \
              /D << /Order [4 0 R 5 0 R] /OFF [5 0 R] >> >> >>"
                .to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>".to_vec(),
            b"<< /Type /OCG /Name (Floor plan) >>".to_vec(),
            b"<< /Type /OCG /Name (Wiring) >>".to_vec(),
        ];
        assemble(&objects)
    }

    fn assemble(objects: &[Vec<u8>]) -> Vec<u8> {
        use std::io::Write as _;
        let mut bytes: Vec<u8> = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (index, body) in objects.iter().enumerate() {
            offsets.push(bytes.len());
            write!(bytes, "{} 0 obj\n", index + 1).unwrap();
            bytes.extend_from_slice(body);
            bytes.extend_from_slice(b"\nendobj\n");
        }
        let xref = bytes.len();
        write!(bytes, "xref\n0 {}\n", objects.len() + 1).unwrap();
        bytes.extend_from_slice(b"0000000000 65535 f \n");
        for offset in &offsets {
            writeln!(bytes, "{offset:010} 00000 n ").unwrap();
        }
        write!(
            bytes,
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .unwrap();
        bytes
    }

    #[test]
    fn layers_are_read_with_their_names_and_default_visibility() {
        let groups = parse_optional_content_groups(&document_with_layers());

        assert_eq!(groups.len(), 2, "both groups must be found: {groups:?}");
        assert_eq!(groups[0].name, "Floor plan");
        assert!(groups[0].visible, "a group not in /OFF is on");
        assert_eq!(groups[1].name, "Wiring");
        assert!(!groups[1].visible, "a group listed in /OFF is off");
    }

    #[test]
    fn a_document_without_optional_content_reports_none() {
        let objects: Vec<Vec<u8>> = vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>".to_vec(),
        ];
        assert!(parse_optional_content_groups(&assemble(&objects)).is_empty());
    }

    #[test]
    fn base_state_off_inverts_the_default() {
        let objects: Vec<Vec<u8>> = vec![
            b"<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [4 0 R 5 0 R] \
              /D << /BaseState /OFF /ON [5 0 R] >> >> >>"
                .to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>".to_vec(),
            b"<< /Type /OCG /Name (Hidden by default) >>".to_vec(),
            b"<< /Type /OCG /Name (Explicitly on) >>".to_vec(),
        ];
        let groups = parse_optional_content_groups(&assemble(&objects));

        assert_eq!(groups.len(), 2);
        assert!(!groups[0].visible, "/BaseState /OFF hides what /ON omits");
        assert!(groups[1].visible, "/ON turns its groups back on");
    }

    #[test]
    fn a_name_with_an_escaped_parenthesis_survives() {
        let objects: Vec<Vec<u8>> = vec![
            b"<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [4 0 R] /D << >> >> >>"
                .to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>".to_vec(),
            br"<< /Type /OCG /Name (Level \(basement\)) >>".to_vec(),
        ];
        let groups = parse_optional_content_groups(&assemble(&objects));

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].name, "Level (basement)");
    }
}
