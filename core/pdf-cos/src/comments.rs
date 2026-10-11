//! Comments (annotations that carry review content) read from the document's
//! own objects. [FR-REV-2, FR-REV-3, US-STU-7]
//!
//! A comment summary has to work on documents this product did not author:
//! whatever Acrobat, a browser or a reviewer left in `/Annots`. It reads the
//! page tree in order, so each comment carries the page a reader would see.
//! Content is returned as text; nothing here renders or executes anything.

use crate::attachments::{array_after, read_string, refs_in, Fetch};
use crate::scan::{
    fetch_object_bytes, find_indirect_ref, find_key, find_startxref, find_trailer, parse_xref_chain,
    skip_ws,
};

/// Page-tree nodes visited before giving up. [GR-7]
const MAX_NODES: usize = 100_000;
const MAX_DEPTH: usize = 64;

/// One comment as the document declares it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment {
    /// 1-based page number in reading order.
    pub page: u32,
    /// `Text`, `Highlight`, `FreeText`, ...
    pub subtype: String,
    /// `/T`, the author.
    pub author: String,
    /// `/Contents`, the comment's text. May be empty (a bare highlight).
    pub contents: String,
    /// `/M`, the modification date exactly as written (a PDF date string).
    pub modified: String,
    /// Whether this annotation replies to another (`/IRT`).
    pub is_reply: bool,
}

/// Subtypes that are not comments: navigation, widgets and popup windows.
const NOT_COMMENTS: [&str; 4] = ["Link", "Popup", "Widget", "Screen"];

/// All comments in page order. Empty for a document with none, and for one
/// whose cross-reference data cannot be read.
#[must_use]
pub fn list_comments(data: &[u8]) -> Vec<Comment> {
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
    let Some((pages_root, _)) = find_indirect_ref(&catalog, b"/Pages") else {
        return Vec::new();
    };

    let mut leaves = Vec::new();
    let mut visited = 0usize;
    collect_leaves(pages_root, &fetch, 0, &mut visited, &mut leaves);

    let mut comments = Vec::new();
    for (index, page) in leaves.iter().enumerate() {
        for annotation in annotations_of(page, &fetch) {
            if let Some(comment) = to_comment(&annotation, index as u32 + 1) {
                comments.push(comment);
            }
        }
    }
    comments
}

fn collect_leaves(
    node_num: u32,
    fetch: Fetch,
    depth: usize,
    visited: &mut usize,
    out: &mut Vec<Vec<u8>>,
) {
    *visited += 1;
    if depth > MAX_DEPTH || *visited > MAX_NODES {
        return;
    }
    let Some(node) = fetch(node_num) else {
        return;
    };
    match array_after(&node, b"/Kids") {
        Some(kids) => {
            for child in refs_in(&kids) {
                collect_leaves(child, fetch, depth + 1, visited, out);
            }
        }
        None => out.push(node),
    }
}

/// The annotation dictionaries a page lists, whether `/Annots` is an inline
/// array or a reference to one.
fn annotations_of(page: &[u8], fetch: Fetch) -> Vec<Vec<u8>> {
    let array = match find_indirect_ref(page, b"/Annots") {
        Some((num, _)) => fetch(num).and_then(|body| {
            let start = body.iter().position(|&b| b == b'[')?;
            let end = body.iter().rposition(|&b| b == b']')?;
            body.get(start + 1..end).map(<[u8]>::to_vec)
        }),
        None => array_after(page, b"/Annots"),
    };
    array
        .map(|a| refs_in(&a))
        .unwrap_or_default()
        .into_iter()
        .filter_map(fetch)
        .collect()
}

fn to_comment(annotation: &[u8], page: u32) -> Option<Comment> {
    let subtype = name_value(annotation, b"/Subtype")?;
    if NOT_COMMENTS.contains(&subtype.as_str()) {
        return None;
    }
    let text = |key: &[u8]| string_value(annotation, key).unwrap_or_default();
    Some(Comment {
        page,
        subtype,
        author: text(b"/T"),
        contents: text(b"/Contents"),
        modified: text(b"/M"),
        is_reply: key_position(annotation, b"/IRT").is_some(),
    })
}

/// Position just past `key`, requiring the next byte to end the name: `/T`
/// must not match inside `/Type`, `/Title` or `/TU`.
fn key_position(dict: &[u8], key: &[u8]) -> Option<usize> {
    let mut from = 0;
    while let Some(found) = find_key(&dict[from..], key) {
        let end = from + found + key.len();
        match dict.get(end) {
            None => return Some(end),
            Some(b) if b.is_ascii_whitespace() || matches!(b, b'(' | b'<' | b'/' | b'[') => {
                return Some(end)
            }
            _ => from = end,
        }
    }
    None
}

fn string_value(dict: &[u8], key: &[u8]) -> Option<String> {
    let mut i = key_position(dict, key)?;
    skip_ws(dict, &mut i);
    read_string(dict, &mut i)
}

fn name_value(dict: &[u8], key: &[u8]) -> Option<String> {
    let mut i = key_position(dict, key)?;
    skip_ws(dict, &mut i);
    if dict.get(i) != Some(&b'/') {
        return None;
    }
    i += 1;
    let start = i;
    while i < dict.len()
        && !dict[i].is_ascii_whitespace()
        && !matches!(dict[i], b'/' | b'>' | b'<' | b'[' | b']' | b'(' | b')')
    {
        i += 1;
    }
    Some(String::from_utf8_lossy(&dict[start..i]).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pdf(objects: &[&str]) -> Vec<u8> {
        let mut out = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (i, body) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend(format!("{} 0 obj\n{body}\nendobj\n", i + 1).bytes());
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

    /// Two pages under a nested tree; page 2 carries a note, a reply, a link
    /// (not a comment) and a popup (not a comment); `/Annots` on page 2 is an
    /// indirect array, on page 1 inline.
    fn document() -> Vec<u8> {
        pdf(&[
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [3 0 R 6 0 R] /Count 2 >>",
            "<< /Type /Pages /Parent 2 0 R /Kids [4 0 R] /Count 1 >>",
            "<< /Type /Page /Parent 3 0 R /Annots [5 0 R] >>",
            "<< /Type /Annot /Subtype /Highlight /T (Ana) /M (D:20260101120000Z) /Rect [0 0 1 1] >>",
            "<< /Type /Page /Parent 2 0 R /Annots 7 0 R >>",
            "[ 8 0 R 9 0 R 10 0 R 11 0 R 12 0 R ]",
            "<< /Type /Annot /Subtype /Text /T (Ben) /Contents (Check \\(this\\) figure) /Rect [0 0 1 1] >>",
            "<< /Type /Annot /Subtype /FreeText /T (Ana) /Contents <FEFF004F004B> /IRT 8 0 R >>",
            "<< /Type /Annot /Subtype /Link /Rect [0 0 1 1] /A << /S /URI >> >>",
            "<< /Type /Annot /Subtype /Popup /Parent 8 0 R >>",
            "<< /Type /Annot /Subtype /Widget /T (field) >>",
        ])
    }

    #[test]
    fn comments_come_out_in_page_order_with_author_text_and_replies() {
        let got = list_comments(&document());
        let summary: Vec<(u32, &str, &str, &str, bool)> = got
            .iter()
            .map(|c| (c.page, c.subtype.as_str(), c.author.as_str(), c.contents.as_str(), c.is_reply))
            .collect();
        assert_eq!(
            summary,
            [
                (1, "Highlight", "Ana", "", false),
                (2, "Text", "Ben", "Check (this) figure", false),
                (2, "FreeText", "Ana", "OK", true),
            ]
        );
        assert_eq!(got[0].modified, "D:20260101120000Z");
    }

    #[test]
    fn the_author_key_does_not_match_inside_type() {
        // /Title comes first and is a different key: the author is /T alone.
        let bare = pdf(&[
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            "<< /Type /Page /Parent 2 0 R /Annots [4 0 R] >>",
            "<< /Type /Annot /Subtype /Text /Title (Wrong) /T (Right) /Contents (hi) >>",
        ]);
        let got = list_comments(&bare);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].author, "Right");
    }

    #[test]
    fn a_document_with_no_annotations_has_no_comments() {
        let plain = pdf(&[
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            "<< /Type /Page /Parent 2 0 R >>",
        ]);
        assert!(list_comments(&plain).is_empty());
    }
}
