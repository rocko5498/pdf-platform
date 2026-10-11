//! Text of a tagged document in its logical reading order.
//! [FR-A11Y-1, FR-A11Y-3, FR-EXPORT-1, US-AXS-2, ADR-005]
//!
//! A tagged PDF carries a structure tree (`/StructTreeRoot`) whose leaves name
//! pieces of page content by marked-content id (MCID). The order the tree
//! lists them in is the order a person is meant to read, and it can differ
//! from the order the content stream draws them in (columns, sidebars, running
//! headers). The engine's text extraction follows the drawing order, so this
//! module reads the tree and the content streams itself.
//!
//! What it does: walk the tree in order, find each MCID in the page's content
//! stream, decode the text shown inside it, and emit it element by element.
//!
//! What it does not do, and says so: a font without a `/ToUnicode` map is
//! decoded as Latin-1 for simple fonts and not at all for composite ones, and
//! every such case sets [`ReadingOrder::unreliable`] instead of passing a guess
//! off as text (PRIN-6). Layout inside an element is approximate: a text-
//! positioning move down a line becomes a line break, a wide gap in a `TJ`
//! array becomes a space.

use std::collections::BTreeMap;

use crate::attachments::{array_after, dict_value, refs_in, Fetch};
use crate::scan::{
    fetch_object_bytes, find_indirect_ref, find_key, find_startxref, section_dictionary, parse_xref_chain,
    skip_ws,
};
use crate::xref::decode_stream_limited;

/// Structure-tree elements visited before giving up. [GR-7]
const MAX_ELEMENTS: usize = 200_000;
const MAX_DEPTH: usize = 128;
/// Decoded content-stream bytes read per page. [GR-7]
const MAX_PAGE_STREAM: usize = 32 * 1024 * 1024;

/// One structure element's text, in reading order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadingBlock {
    /// 1-based page of the element's first content.
    pub page: u32,
    /// The structure type: `P`, `H1`, `Figure`, `TD`, ...
    pub element: String,
    /// The element's text (its `/Alt` text for a figure with no text).
    pub text: String,
}

/// The document's text in logical order, with what could not be decoded.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReadingOrder {
    /// Elements that carry text, in the order the structure tree lists them.
    pub blocks: Vec<ReadingBlock>,
    /// Content references (MCIDs) the tree names that were not found on their
    /// page, for instance because the page stream could not be decoded.
    pub missing_content: u32,
    /// Some text was decoded without a Unicode map and may be wrong.
    pub unreliable: bool,
}

/// Why no reading order could be produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadingOrderError {
    /// The cross-reference data could not be read.
    Unreadable,
    /// The catalog has no `/StructTreeRoot`: the document is not tagged.
    NotTagged,
}

impl std::fmt::Display for ReadingOrderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreadable => write!(f, "the document's structure could not be read"),
            Self::NotTagged => write!(f, "the document has no structure tree (it is not tagged)"),
        }
    }
}

/// Read the document's text in reading order.
///
/// # Errors
/// [`ReadingOrderError::NotTagged`] when there is no structure tree, and
/// [`ReadingOrderError::Unreadable`] when the file cannot be read at all.
pub fn read_in_order(data: &[u8]) -> Result<ReadingOrder, ReadingOrderError> {
    let xref_offset = find_startxref(data).ok_or(ReadingOrderError::Unreadable)?;
    let mut leniency = Vec::new();
    let xref = parse_xref_chain(data, xref_offset, &mut leniency)
        .map_err(|_| ReadingOrderError::Unreadable)?;
    let trailer = section_dictionary(data, xref_offset).ok_or(ReadingOrderError::Unreadable)?;
    let (root, _) = find_indirect_ref(&trailer, b"/Root").ok_or(ReadingOrderError::Unreadable)?;
    let fetch = |num: u32| fetch_object_bytes(data, &xref, num);
    let catalog = fetch(root).ok_or(ReadingOrderError::Unreadable)?;
    let (struct_root, _) =
        find_indirect_ref(&catalog, b"/StructTreeRoot").ok_or(ReadingOrderError::NotTagged)?;
    let (pages_root, _) = find_indirect_ref(&catalog, b"/Pages").ok_or(ReadingOrderError::Unreadable)?;

    // Pages by object number, with their number and inherited resources.
    let mut pages: BTreeMap<u32, PageInfo> = BTreeMap::new();
    let mut visited = 0usize;
    collect_pages(pages_root, None, &fetch, 0, &mut visited, &mut pages);

    // The tree: ordered (element, page object, mcid) references.
    let mut refs: Vec<ContentRef> = Vec::new();
    let mut seen = 0usize;
    if let Some(node) = fetch(struct_root) {
        walk_struct(&node, None, "Document", &fetch, 0, &mut seen, &mut refs);
    }

    // Text of each (page, mcid), decoded once per page.
    let mut decoded: BTreeMap<u32, PageText> = BTreeMap::new();
    let mut out = ReadingOrder::default();
    let mut current: Option<(String, u32, Vec<String>)> = None;
    let mut flush = |current: &mut Option<(String, u32, Vec<String>)>, out: &mut ReadingOrder| {
        if let Some((element, page, parts)) = current.take() {
            let text = parts.iter().map(|p| p.trim()).filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" ");
            if !text.is_empty() {
                out.blocks.push(ReadingBlock { page, element, text });
            }
        }
    };

    for r in &refs {
        if let Some(alt) = &r.alt {
            // A figure with alternative text and no marked content of its own.
            flush(&mut current, &mut out);
            if let Some(info) = r.page_obj.and_then(|p| pages.get(&p)) {
                out.blocks.push(ReadingBlock {
                    page: info.number,
                    element: r.element.clone(),
                    text: format!("[{}: {alt}]", r.element),
                });
            }
            continue;
        }
        let (Some(page_obj), Some(mcid)) = (r.page_obj, r.mcid) else {
            continue;
        };
        let Some(info) = pages.get(&page_obj) else {
            out.missing_content += 1;
            continue;
        };
        let text = decoded
            .entry(page_obj)
            .or_insert_with(|| decode_page(info, &fetch));
        if text.unreliable {
            out.unreliable = true;
        }
        let Some(piece) = text.by_mcid.get(&mcid) else {
            if !text.seen.contains(&mcid) {
                out.missing_content += 1;
            }
            continue;
        };
        match &mut current {
            Some((element, _, parts)) if *element == r.element && r.same_element_as_previous => {
                parts.push(piece.clone());
            }
            _ => {
                flush(&mut current, &mut out);
                current = Some((r.element.clone(), info.number, vec![piece.clone()]));
            }
        }
    }
    flush(&mut current, &mut out);
    Ok(out)
}

// ---------------------------------------------------------------------------
// Page tree
// ---------------------------------------------------------------------------

struct PageInfo {
    number: u32,
    body: Vec<u8>,
    /// `/Resources` found on the page or inherited from an ancestor.
    resources: Option<Vec<u8>>,
}

fn collect_pages(
    num: u32,
    inherited: Option<Vec<u8>>,
    fetch: Fetch,
    depth: usize,
    visited: &mut usize,
    out: &mut BTreeMap<u32, PageInfo>,
) {
    *visited += 1;
    if depth > MAX_DEPTH || *visited > MAX_ELEMENTS {
        return;
    }
    let Some(node) = fetch(num) else {
        return;
    };
    let own = dict_value(&node, b"/Resources", fetch).or(inherited);
    match array_after(&node, b"/Kids") {
        Some(kids) => {
            for child in refs_in(&kids) {
                collect_pages(child, own.clone(), fetch, depth + 1, visited, out);
            }
        }
        None => {
            let number = out.len() as u32 + 1;
            out.insert(num, PageInfo { number, body: node, resources: own });
        }
    }
}

// ---------------------------------------------------------------------------
// Structure tree
// ---------------------------------------------------------------------------

struct ContentRef {
    element: String,
    page_obj: Option<u32>,
    mcid: Option<u32>,
    alt: Option<String>,
    /// Whether this reference belongs to the same structure element as the
    /// one before it (so their text joins into one block).
    same_element_as_previous: bool,
}

fn walk_struct(
    node: &[u8],
    inherited_page: Option<u32>,
    inherited_tag: &str,
    fetch: Fetch,
    depth: usize,
    seen: &mut usize,
    out: &mut Vec<ContentRef>,
) {
    *seen += 1;
    if depth > MAX_DEPTH || *seen > MAX_ELEMENTS {
        return;
    }
    let tag = name_after(node, b"/S").unwrap_or_else(|| inherited_tag.to_string());
    let page = find_indirect_ref(node, b"/Pg").map(|(n, _)| n).or(inherited_page);
    let alt = string_after(node, b"/Alt").filter(|a| !a.is_empty());
    let before = out.len();

    if let Some(k) = kids_value(node) {
        walk_kids(&k, page, &tag, fetch, depth, seen, out);
    }
    // A figure (or any element) with alternative text and no content of its own.
    if out.len() == before {
        if let Some(alt) = alt {
            out.push(ContentRef {
                element: tag,
                page_obj: page,
                mcid: None,
                alt: Some(alt),
                same_element_as_previous: false,
            });
        }
    }
}

/// The `/K` entry: an array, a single value, or an indirect reference to either.
fn kids_value(node: &[u8]) -> Option<Vec<u8>> {
    let pos = find_key(node, b"/K")?;
    // `/K` must be the whole key, not the start of `/Kids`.
    if node.get(pos + 2).is_some_and(|b| b.is_ascii_alphanumeric()) {
        return None;
    }
    let rest = &node[pos + 2..];
    let mut i = 0;
    skip_ws(rest, &mut i);
    match rest.get(i)? {
        b'[' => array_after(node, b"/K"),
        _ => {
            // One value: take up to the next top-level key.
            let end = rest[i..]
                .windows(2)
                .position(|w| w[0] == b'/' && w[1].is_ascii_alphabetic())
                .map_or(rest.len(), |p| i + p);
            Some(rest[i..end].to_vec())
        }
    }
}

fn walk_kids(
    kids: &[u8],
    page: Option<u32>,
    tag: &str,
    fetch: Fetch,
    depth: usize,
    seen: &mut usize,
    out: &mut Vec<ContentRef>,
) {
    let mut i = 0;
    let mut first_in_element = true;
    while i < kids.len() {
        skip_ws(kids, &mut i);
        let Some(&b) = kids.get(i) else {
            break;
        };
        if b == b'<' && kids.get(i + 1) == Some(&b'<') {
            // Inline dictionary: a marked-content reference or an inline element.
            let Some(dict) = balanced(kids, &mut i) else {
                break;
            };
            let is_mcr = find_key(&dict, b"/MCID").is_some();
            if is_mcr {
                let mcr_page = find_indirect_ref(&dict, b"/Pg").map(|(n, _)| n).or(page);
                out.push(ContentRef {
                    element: tag.to_string(),
                    page_obj: mcr_page,
                    mcid: int_after(&dict, b"/MCID"),
                    alt: None,
                    same_element_as_previous: !first_in_element,
                });
                first_in_element = false;
            } else {
                walk_struct(&dict, page, tag, fetch, depth + 1, seen, out);
                first_in_element = true;
            }
        } else if b.is_ascii_digit() {
            // Either `N G R` (a child element) or a bare integer (an MCID).
            let num = read_uint(kids, &mut i);
            skip_ws(kids, &mut i);
            let after_num = i;
            let gen_ok = read_uint(kids, &mut i).is_some();
            skip_ws(kids, &mut i);
            if gen_ok && kids.get(i) == Some(&b'R') {
                i += 1;
                if let Some(child) = num.and_then(fetch) {
                    walk_struct(&child, page, tag, fetch, depth + 1, seen, out);
                }
                first_in_element = true;
            } else {
                i = after_num;
                out.push(ContentRef {
                    element: tag.to_string(),
                    page_obj: page,
                    mcid: num,
                    alt: None,
                    same_element_as_previous: !first_in_element,
                });
                first_in_element = false;
            }
        } else {
            i += 1;
        }
    }
}

fn read_uint(data: &[u8], i: &mut usize) -> Option<u32> {
    let start = *i;
    while *i < data.len() && data[*i].is_ascii_digit() {
        *i += 1;
    }
    std::str::from_utf8(&data[start..*i]).ok()?.parse().ok()
}

fn balanced(data: &[u8], i: &mut usize) -> Option<Vec<u8>> {
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

// ---------------------------------------------------------------------------
// Page content
// ---------------------------------------------------------------------------

#[derive(Default)]
struct PageText {
    by_mcid: BTreeMap<u32, String>,
    /// MCIDs the stream opens at all, with or without text. A rule, a bullet
    /// or an image is a real marked-content item that shows no text; only an
    /// MCID the stream never opens is missing.
    seen: std::collections::BTreeSet<u32>,
    unreliable: bool,
}

fn decode_page(page: &PageInfo, fetch: Fetch) -> PageText {
    let mut stream = Vec::new();
    let contents = contents_refs(&page.body);
    for num in contents {
        if let Some(object) = fetch(num) {
            if let Ok(bytes) = decode_stream_limited(&object, MAX_PAGE_STREAM) {
                stream.extend_from_slice(&bytes);
                stream.push(b'\n');
            }
        }
    }
    let mut out = PageText::default();
    let mut stack: Vec<Option<u32>> = Vec::new();
    run(&stream, page.resources.as_deref(), fetch, &mut out, &mut stack, 0);
    // Tidy: collapse runs of blank lines.
    for text in out.by_mcid.values_mut() {
        let cleaned: Vec<&str> = text.lines().map(str::trim_end).filter(|l| !l.is_empty()).collect();
        *text = cleaned.join("\n");
    }
    out
}

/// How deep `Do` may nest Form XObjects. A form that draws itself, or a chain
/// of them, ends here instead of looping. [GR-7]
const MAX_FORM_DEPTH: usize = 8;

/// The Form XObject a `Do` names: its decoded content and its own resources
/// (or the caller's, when it declares none).
fn load_form(
    name: &str,
    resources: Option<&[u8]>,
    fetch: Fetch,
) -> Option<(Vec<u8>, Option<Vec<u8>>)> {
    let xobjects = spaced(&dict_value(resources?, b"/XObject", fetch)?);
    let tokens: Vec<&str> = xobjects.split_whitespace().collect();
    let num = tokens.windows(4).find_map(|w| {
        (w[0].strip_prefix('/') == Some(name) && w[3] == "R").then(|| w[1].parse::<u32>().ok())?
    })?;
    let object = fetch(num)?;
    if name_after(&object, b"/Subtype").as_deref() != Some("Form") {
        return None;
    }
    let content = decode_stream_limited(&object, MAX_PAGE_STREAM).ok()?;
    let own = dict_value(&object, b"/Resources", fetch).or_else(|| resources.map(<[u8]>::to_vec));
    Some((content, own))
}

/// `/Properties` resource entries that carry an MCID, by name: a content stream
/// may write `/P /MC0 BDC` and leave the MCID in the resource dictionary.
fn load_properties(resources: Option<&[u8]>, fetch: Fetch) -> BTreeMap<String, u32> {
    let mut map = BTreeMap::new();
    let Some(dict) = resources.and_then(|r| dict_value(r, b"/Properties", fetch)) else {
        return map;
    };
    let text = spaced(&dict);
    let tokens: Vec<&str> = text.split_whitespace().collect();
    for w in tokens.windows(4) {
        if let (Some(name), Ok(num), "R") = (w[0].strip_prefix('/'), w[1].parse::<u32>(), w[3]) {
            if let Some(mcid) = fetch(num).and_then(|body| int_after(&body, b"/MCID")) {
                map.insert(name.to_string(), mcid);
            }
        }
    }
    map
}

fn contents_refs(page: &[u8]) -> Vec<u32> {
    if let Some(array) = array_after(page, b"/Contents") {
        return refs_in(&array);
    }
    find_indirect_ref(page, b"/Contents").map(|(n, _)| vec![n]).unwrap_or_default()
}

#[derive(Debug, Clone)]
enum Operand {
    Num(f64),
    Name(String),
    Str(Vec<u8>),
    Array(Vec<Operand>),
    Dict(Vec<(String, Operand)>),
}

/// Interpret one content stream (a page's, or a Form XObject's) against its
/// own resources. Marked content opened outside a form keeps applying inside
/// it, because `stack` and `out` are shared with the caller.
fn run(
    stream: &[u8],
    resources: Option<&[u8]>,
    fetch: Fetch,
    out: &mut PageText,
    stack: &mut Vec<Option<u32>>,
    depth: usize,
) {
    let fonts = FontTable::load(resources, fetch);
    let properties = load_properties(resources, fetch);
    let mut i = 0;
    let mut operands: Vec<Operand> = Vec::new();
    let mut font: Option<String> = None;

    while i < stream.len() {
        skip_content_ws(stream, &mut i);
        if i >= stream.len() {
            break;
        }
        let Some(token) = next_operand_or_op(stream, &mut i) else {
            break;
        };
        match token {
            Token::Operand(o) => operands.push(o),
            Token::Op(op) => {
                match op.as_str() {
                    "BDC" => {
                        let mcid = match operands.last() {
                            Some(Operand::Dict(entries)) => entries
                                .iter()
                                .find(|(k, _)| k == "MCID")
                                .and_then(|(_, v)| if let Operand::Num(n) = v { Some(*n as u32) } else { None }),
                            Some(Operand::Name(name)) => properties.get(name).copied(),
                            _ => None,
                        };
                        if let Some(id) = mcid {
                            out.seen.insert(id);
                        }
                        stack.push(mcid);
                    }
                    "BMC" => stack.push(None),
                    "EMC" => {
                        stack.pop();
                    }
                    "Tf" => {
                        if let Some(Operand::Name(n)) = operands.iter().rev().nth(1) {
                            font = Some(n.clone());
                        }
                    }
                    "Tj" | "'" | "\"" => {
                        if op != "Tj" {
                            push_text(out, stack, "\n");
                        }
                        if let Some(Operand::Str(s)) = operands.last() {
                            show(out, stack, &fonts, font.as_deref(), s);
                        }
                    }
                    "TJ" => {
                        if let Some(Operand::Array(items)) = operands.last() {
                            for item in items {
                                match item {
                                    Operand::Str(s) => show(out, stack, &fonts, font.as_deref(), s),
                                    Operand::Num(n) if *n < -200.0 => push_text(out, stack, " "),
                                    _ => {}
                                }
                            }
                        }
                    }
                    "Td" | "TD" => {
                        if let Some(Operand::Num(ty)) = operands.last() {
                            if ty.abs() > 0.5 {
                                push_text(out, stack, "\n");
                            }
                        }
                    }
                    "T*" | "Tm" => push_text(out, stack, "\n"),
                    "Do" => {
                        if let (Some(Operand::Name(name)), true) = (operands.last(), depth < MAX_FORM_DEPTH) {
                            if let Some((content, own)) = load_form(name, resources, fetch) {
                                run(&content, own.as_deref(), fetch, out, stack, depth + 1);
                            }
                        }
                    }
                    _ => {}
                }
                operands.clear();
            }
        }
    }
}

fn push_text(out: &mut PageText, stack: &[Option<u32>], text: &str) {
    if let Some(mcid) = stack.iter().rev().find_map(|m| *m) {
        let entry = out.by_mcid.entry(mcid).or_default();
        if text == "\n" && (entry.is_empty() || entry.ends_with('\n')) {
            return;
        }
        entry.push_str(text);
    }
}

fn show(out: &mut PageText, stack: &[Option<u32>], fonts: &FontTable, font: Option<&str>, bytes: &[u8]) {
    let Some(mcid) = stack.iter().rev().find_map(|m| *m) else {
        return;
    };
    let (text, reliable) = fonts.decode(font, bytes);
    if !reliable {
        out.unreliable = true;
    }
    out.by_mcid.entry(mcid).or_default().push_str(&text);
}

enum Token {
    Operand(Operand),
    Op(String),
}

fn skip_content_ws(data: &[u8], i: &mut usize) {
    while *i < data.len() {
        match data[*i] {
            b' ' | b'\t' | b'\r' | b'\n' | 0 | 12 => *i += 1,
            b'%' => {
                while *i < data.len() && data[*i] != b'\n' && data[*i] != b'\r' {
                    *i += 1;
                }
            }
            _ => break,
        }
    }
}

fn is_delimiter(b: u8) -> bool {
    matches!(b, b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%')
}

fn next_operand_or_op(data: &[u8], i: &mut usize) -> Option<Token> {
    skip_content_ws(data, i);
    let b = *data.get(*i)?;
    match b {
        b'/' => {
            *i += 1;
            let start = *i;
            while *i < data.len() && !data[*i].is_ascii_whitespace() && !is_delimiter(data[*i]) {
                *i += 1;
            }
            Some(Token::Operand(Operand::Name(String::from_utf8_lossy(&data[start..*i]).into_owned())))
        }
        b'(' => Some(Token::Operand(Operand::Str(literal_string(data, i)))),
        b'<' if data.get(*i + 1) == Some(&b'<') => {
            *i += 2;
            let mut entries = Vec::new();
            loop {
                skip_content_ws(data, i);
                if data.get(*i..*i + 2) == Some(b">>") {
                    *i += 2;
                    break;
                }
                if *i >= data.len() {
                    break;
                }
                let key = match next_operand_or_op(data, i)? {
                    Token::Operand(Operand::Name(k)) => k,
                    _ => continue,
                };
                match next_operand_or_op(data, i)? {
                    Token::Operand(v) => entries.push((key, v)),
                    Token::Op(_) => {}
                }
            }
            Some(Token::Operand(Operand::Dict(entries)))
        }
        b'<' => {
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
            let bytes = digits
                .chunks(2)
                .filter_map(|p| u8::from_str_radix(std::str::from_utf8(p).ok()?, 16).ok())
                .collect();
            Some(Token::Operand(Operand::Str(bytes)))
        }
        b'[' => {
            *i += 1;
            let mut items = Vec::new();
            loop {
                skip_content_ws(data, i);
                if data.get(*i) == Some(&b']') {
                    *i += 1;
                    break;
                }
                if *i >= data.len() {
                    break;
                }
                match next_operand_or_op(data, i)? {
                    Token::Operand(o) => items.push(o),
                    Token::Op(_) => {}
                }
            }
            Some(Token::Operand(Operand::Array(items)))
        }
        b']' | b'>' | b')' | b'{' | b'}' => {
            *i += 1;
            next_operand_or_op(data, i)
        }
        _ => {
            let start = *i;
            while *i < data.len() && !data[*i].is_ascii_whitespace() && !is_delimiter(data[*i]) {
                *i += 1;
            }
            let word = std::str::from_utf8(&data[start..*i]).ok()?;
            match word.parse::<f64>() {
                Ok(n) => Some(Token::Operand(Operand::Num(n))),
                Err(_) => Some(Token::Op(word.to_string())),
            }
        }
    }
}

fn literal_string(data: &[u8], i: &mut usize) -> Vec<u8> {
    *i += 1;
    let mut out = Vec::new();
    let mut depth = 1usize;
    while *i < data.len() {
        let c = data[*i];
        *i += 1;
        match c {
            b'\\' => {
                let Some(&e) = data.get(*i) else {
                    break;
                };
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

// ---------------------------------------------------------------------------
// Fonts
// ---------------------------------------------------------------------------

struct FontDecoder {
    /// `/ToUnicode` entries by character code, and the code width in bytes.
    to_unicode: Option<(BTreeMap<u32, String>, usize)>,
    composite: bool,
}

struct FontTable {
    fonts: BTreeMap<String, FontDecoder>,
}

impl FontTable {
    fn load(resources: Option<&[u8]>, fetch: Fetch) -> Self {
        let mut fonts = BTreeMap::new();
        let Some(resources) = resources else {
            return Self { fonts };
        };
        let Some(font_dict) = dict_value(resources, b"/Font", fetch) else {
            return Self { fonts };
        };
        // `/F1 12 0 R /F2 13 0 R`
        let text = spaced(&font_dict);
        let tokens: Vec<&str> = text.split_whitespace().collect();
        for w in tokens.windows(4) {
            if let (Some(name), Ok(num), "R") = (w[0].strip_prefix('/'), w[1].parse::<u32>(), w[3]) {
                if w[2].parse::<u32>().is_ok() {
                    if let Some(body) = fetch(num) {
                        fonts.insert(name.to_string(), FontDecoder::from_dict(&body, fetch));
                    }
                }
            }
        }
        Self { fonts }
    }

    fn decode(&self, font: Option<&str>, bytes: &[u8]) -> (String, bool) {
        let decoder = font.and_then(|f| self.fonts.get(f));
        match decoder {
            Some(FontDecoder { to_unicode: Some((map, width)), .. }) => {
                let mut text = String::new();
                let mut reliable = true;
                for chunk in bytes.chunks(*width) {
                    let code = chunk.iter().fold(0u32, |a, b| (a << 8) | u32::from(*b));
                    match map.get(&code) {
                        Some(s) => text.push_str(s),
                        None => {
                            text.push('\u{FFFD}');
                            reliable = false;
                        }
                    }
                }
                (text, reliable)
            }
            Some(FontDecoder { composite: true, .. }) => {
                // Two-byte codes with no Unicode map: nothing sound to say.
                ("\u{FFFD}".repeat(bytes.len() / 2), false)
            }
            // A simple font with no /ToUnicode: Latin-1 is right for ASCII and
            // close for WinAnsi text, and wrong for custom encodings.
            _ => (bytes.iter().map(|b| char::from(*b)).collect(), decoder.is_some() && bytes.is_ascii()),
        }
    }
}

impl FontDecoder {
    fn from_dict(body: &[u8], fetch: Fetch) -> Self {
        let composite = name_after(body, b"/Subtype").is_some_and(|s| s == "Type0");
        let to_unicode = find_indirect_ref(body, b"/ToUnicode")
            .and_then(|(n, _)| fetch(n))
            .and_then(|object| decode_stream_limited(&object, 4 * 1024 * 1024).ok())
            .map(|cmap| parse_to_unicode(&cmap));
        Self { to_unicode, composite }
    }
}

enum RangeItem {
    Hex(String),
    Open,
    Close,
}

/// The `<hex>`, `[` and `]` tokens of a bfrange section, in order.
fn range_items(section: &str) -> Vec<RangeItem> {
    let mut items = Vec::new();
    let bytes = section.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'<' => {
                let end = section[i..].find('>').map_or(bytes.len(), |e| i + e);
                items.push(RangeItem::Hex(
                    section[i + 1..end].split_whitespace().collect::<String>(),
                ));
                i = end + 1;
            }
            b'[' => {
                items.push(RangeItem::Open);
                i += 1;
            }
            b']' => {
                items.push(RangeItem::Close);
                i += 1;
            }
            _ => i += 1,
        }
    }
    items
}

/// `beginbfchar` / `beginbfrange` sections of a `/ToUnicode` CMap.
fn parse_to_unicode(cmap: &[u8]) -> (BTreeMap<u32, String>, usize) {
    let text = String::from_utf8_lossy(cmap);
    let mut map = BTreeMap::new();
    let mut width = 1usize;
    let hex_items = |section: &str| -> Vec<(String, usize)> {
        let mut items = Vec::new();
        let mut rest = section;
        while let Some(start) = rest.find('<') {
            let Some(end) = rest[start..].find('>') else {
                break;
            };
            let digits = &rest[start + 1..start + end];
            items.push((digits.split_whitespace().collect::<String>(), digits.len()));
            rest = &rest[start + end + 1..];
        }
        items
    };
    let to_text = |hex: &str| -> String {
        let bytes: Vec<u8> = (0..hex.len() / 2 * 2)
            .step_by(2)
            .filter_map(|k| u8::from_str_radix(&hex[k..k + 2], 16).ok())
            .collect();
        let units: Vec<u16> = bytes.chunks_exact(2).map(|p| u16::from_be_bytes([p[0], p[1]])).collect();
        String::from_utf16_lossy(&units)
    };
    for section in text.split("beginbfchar").skip(1) {
        let section = section.split("endbfchar").next().unwrap_or("");
        for pair in hex_items(section).chunks(2) {
            if let [(src, _), (dst, _)] = pair {
                if let Ok(code) = u32::from_str_radix(src, 16) {
                    width = width.max(src.len().div_ceil(2));
                    map.insert(code, to_text(dst));
                }
            }
        }
    }
    for section in text.split("beginbfrange").skip(1) {
        let section = section.split("endbfrange").next().unwrap_or("");
        let mut items = range_items(section).into_iter().peekable();
        while let Some(lo) = items.next() {
            let (RangeItem::Hex(lo), Some(RangeItem::Hex(hi))) = (lo, items.next()) else {
                continue;
            };
            let (Ok(lo_n), Ok(hi_n)) = (u32::from_str_radix(&lo, 16), u32::from_str_radix(&hi, 16)) else {
                continue;
            };
            width = width.max(lo.len().div_ceil(2));
            if hi_n < lo_n || hi_n - lo_n >= 65536 {
                continue;
            }
            match items.next() {
                // <lo> <hi> <dst>: the destination counts up with the code.
                Some(RangeItem::Hex(dst)) => {
                    let first = to_text(&dst).chars().next().map_or(0, |c| c as u32);
                    for (k, code) in (lo_n..=hi_n).enumerate() {
                        if let Some(c) = char::from_u32(first + k as u32) {
                            map.insert(code, c.to_string());
                        }
                    }
                }
                // <lo> <hi> [<d0> <d1> ...]: one destination per code.
                Some(RangeItem::Open) => {
                    let mut code = lo_n;
                    for item in items.by_ref() {
                        match item {
                            RangeItem::Hex(dst) => {
                                if code <= hi_n {
                                    map.insert(code, to_text(&dst));
                                }
                                code += 1;
                            }
                            RangeItem::Close => break,
                            RangeItem::Open => {}
                        }
                    }
                }
                _ => {}
            }
        }
    }
    (map, width.max(1))
}

/// Dictionary text with a space before every `/` and around brackets, so
/// `/F1 5 0 R/F2 9 0 R>>` splits into the tokens a reader expects. Word and
/// most producers write dictionaries with no whitespace between entries.
fn spaced(dict: &[u8]) -> String {
    String::from_utf8_lossy(dict)
        .replace('/', " /")
        .replace(['<', '>', '[', ']'], " ")
}

// ---------------------------------------------------------------------------
// Small readers shared by the tree and the fonts
// ---------------------------------------------------------------------------

fn name_after(dict: &[u8], key: &[u8]) -> Option<String> {
    let mut from = 0;
    loop {
        let found = find_key(&dict[from..], key)?;
        let end = from + found + key.len();
        if dict.get(end).is_some_and(|b| b.is_ascii_alphanumeric()) {
            from = end;
            continue;
        }
        let mut i = end;
        skip_ws(dict, &mut i);
        if dict.get(i) != Some(&b'/') {
            return None;
        }
        i += 1;
        let start = i;
        while i < dict.len() && !dict[i].is_ascii_whitespace() && !is_delimiter(dict[i]) {
            i += 1;
        }
        return Some(String::from_utf8_lossy(&dict[start..i]).into_owned());
    }
}

fn int_after(dict: &[u8], key: &[u8]) -> Option<u32> {
    let pos = find_key(dict, key)? + key.len();
    let mut i = pos;
    skip_ws(dict, &mut i);
    read_uint(dict, &mut i)
}

fn string_after(dict: &[u8], key: &[u8]) -> Option<String> {
    let mut from = 0;
    loop {
        let found = find_key(&dict[from..], key)?;
        let end = from + found + key.len();
        if dict.get(end).is_some_and(|b| b.is_ascii_alphanumeric()) {
            from = end;
            continue;
        }
        let mut i = end;
        skip_ws(dict, &mut i);
        return match dict.get(i)? {
            b'(' => Some(decode_text(&literal_string(dict, &mut i))),
            b'<' => match next_operand_or_op(dict, &mut i)? {
                Token::Operand(Operand::Str(b)) => Some(decode_text(&b)),
                _ => None,
            },
            _ => None,
        };
    }
}

fn decode_text(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let units: Vec<u16> = bytes[2..].chunks_exact(2).map(|p| u16::from_be_bytes([p[0], p[1]])).collect();
        return String::from_utf16_lossy(&units);
    }
    String::from_utf8_lossy(bytes).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(extra: &str, body: &str) -> String {
        format!("<< {extra} /Length {} >>\nstream\n{body}\nendstream", body.len())
    }

    fn pdf(objects: &[String]) -> Vec<u8> {
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
            format!("trailer\n<</Size {} /Root 1 0 R>>\nstartxref\n{xref}\n%%EOF", objects.len() + 1).bytes(),
        );
        out
    }

    /// The content stream draws MCID 1 before MCID 0, a running footer outside
    /// any MCID, a font with a ToUnicode map, and a TJ with a wide gap. The
    /// structure tree lists the heading, then paragraph 0, then 1, then the
    /// mapped text, then a figure that has only alternative text.
    fn tagged_document(with_struct_root: bool) -> Vec<u8> {
        let content = "\
/P << /MCID 1 >> BDC BT /F1 12 Tf 50 700 Td (Second paragraph) Tj ET EMC\n\
/P << /MCID 0 >> BDC BT /F1 12 Tf 50 750 Td (First paragraph) Tj ET EMC\n\
/Artifact BMC BT /F1 12 Tf (Page 1) Tj ET EMC\n\
/P << /MCID 2 >> BDC BT /F2 12 Tf <0102> Tj ET EMC\n\
/H1 << /MCID 3 >> BDC BT /F1 12 Tf [(Hea) -300 (ding)] TJ ET EMC\n";
        let to_unicode = "\
1 begincodespacerange <00> <FF> endcodespacerange\n\
2 beginbfchar <01> <0048> <02> <0069> endbfchar";
        let catalog = if with_struct_root {
            "<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 5 0 R /MarkInfo << /Marked true >> >>"
        } else {
            "<< /Type /Catalog /Pages 2 0 R >>"
        };
        pdf(&[
            catalog.to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /Font << /F1 6 0 R /F2 7 0 R >> >> >>".to_string(),
            stream("", content),
            "<< /Type /StructTreeRoot /K [8 0 R 9 0 R 10 0 R 11 0 R 13 0 R] >>".to_string(),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Custom /ToUnicode 12 0 R >>".to_string(),
            "<< /Type /StructElem /S /H1 /P 5 0 R /Pg 3 0 R /K 3 >>".to_string(),
            "<< /Type /StructElem /S /P /P 5 0 R /Pg 3 0 R /K 0 >>".to_string(),
            "<< /Type /StructElem /S /P /P 5 0 R /Pg 3 0 R /K 1 >>".to_string(),
            "<< /Type /StructElem /S /P /P 5 0 R /K [ << /Type /MCR /Pg 3 0 R /MCID 2 >> ] >>".to_string(),
            stream("", to_unicode),
            "<< /Type /StructElem /S /Figure /P 5 0 R /Pg 3 0 R /Alt (Company logo) >>".to_string(),
        ])
    }

    #[test]
    fn text_comes_out_in_the_trees_order_not_the_streams() {
        let order = read_in_order(&tagged_document(true)).expect("tagged");
        let got: Vec<(u32, &str, &str)> = order
            .blocks
            .iter()
            .map(|b| (b.page, b.element.as_str(), b.text.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                (1, "H1", "Hea ding"),
                (1, "P", "First paragraph"),
                (1, "P", "Second paragraph"),
                (1, "P", "Hi"),
                (1, "Figure", "[Figure: Company logo]"),
            ]
        );
        assert_eq!(order.missing_content, 0);
        assert!(!order.unreliable);
        // The footer drawn outside any marked content is not part of the text.
        assert!(order.blocks.iter().all(|b| !b.text.contains("Page 1")));
    }

    #[test]
    fn an_untagged_document_is_refused_not_guessed() {
        assert_eq!(read_in_order(&tagged_document(false)), Err(ReadingOrderError::NotTagged));
    }

    #[test]
    fn content_the_tree_names_but_the_page_lacks_is_counted_and_unmapped_codes_flag_it() {
        // MCID 9 is not on the page; the font has codes the map lacks.
        let content = "/P << /MCID 0 >> BDC BT /F2 12 Tf <0107> Tj ET EMC";
        let to_unicode = "1 beginbfchar <01> <0048> endbfchar";
        let doc = pdf(&[
            "<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 5 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /Font << /F2 6 0 R >> >> >>".to_string(),
            stream("", content),
            "<< /Type /StructTreeRoot /K [7 0 R 8 0 R] >>".to_string(),
            "<< /Type /Font /Subtype /Type1 /ToUnicode 9 0 R >>".to_string(),
            "<< /Type /StructElem /S /P /P 5 0 R /Pg 3 0 R /K 0 >>".to_string(),
            "<< /Type /StructElem /S /P /P 5 0 R /Pg 3 0 R /K 9 >>".to_string(),
            stream("", to_unicode),
        ]);
        let order = read_in_order(&doc).unwrap();
        assert_eq!(order.missing_content, 1);
        assert!(order.unreliable, "an unmapped code must flag the page, not pass as text");
        assert_eq!(order.blocks.len(), 1);
        assert!(order.blocks[0].text.starts_with('H'));
    }

    /// What real producers write, and the hand-built fixtures above do not:
    /// dictionary entries with no whitespace between them, an MCID kept in
    /// `/Properties` and named in the stream, a bfrange whose destination is an
    /// array, and a marked-content item that draws a rule and shows no text.
    #[test]
    fn the_shapes_real_producers_write_are_read() {
        let content = "\
/P /MC0 BDC BT /F1 12 Tf <0102> Tj ET EMC\n\
/Artifact << /MCID 1 >> BDC 0 0 m 10 10 l S EMC\n";
        // Codes 1 and 2 map through an array-form bfrange: 01 -> 'Z', 02 -> 'Y'.
        let to_unicode = "1 beginbfrange <01> <02> [<005A> <0059>] endbfrange";
        let doc = pdf(&[
            "<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 5 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources <</Font<</F0 8 0 R/F1 6 0 R>>/Properties<</MC0 9 0 R>>>> >>".to_string(),
            stream("", content),
            "<< /Type /StructTreeRoot /K [7 0 R 10 0 R] >>".to_string(),
            "<< /Type /Font /Subtype /Type1 /ToUnicode 11 0 R >>".to_string(),
            "<< /Type /StructElem /S /P /P 5 0 R /Pg 3 0 R /K 0 >>".to_string(),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
            "<< /MCID 0 >>".to_string(),
            "<< /Type /StructElem /S /Figure /P 5 0 R /Pg 3 0 R /K 1 >>".to_string(),
            stream("", to_unicode),
        ]);
        let order = read_in_order(&doc).expect("tagged");
        let texts: Vec<&str> = order.blocks.iter().map(|b| b.text.as_str()).collect();
        assert_eq!(texts, ["ZY"], "{order:?}");
        assert_eq!(order.missing_content, 0, "a rule with no text is not missing content");
        assert!(!order.unreliable);
    }

    /// Text drawn by a Form XObject belongs to the marked content that invokes
    /// it, and the form brings its own fonts. A form that invokes itself must
    /// end, not loop.
    #[test]
    fn text_inside_a_form_xobject_is_read_and_a_self_referencing_form_ends() {
        let page = "/P << /MCID 0 >> BDC /Fm0 Do EMC\n/P << /MCID 1 >> BDC /Loop Do EMC";
        let form = "BT /F1 12 Tf (Inside the form) Tj ET";
        let looping = "BT /F1 12 Tf (Once) Tj ET /Loop Do";
        let doc = pdf(&[
            "<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 5 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /XObject << /Fm0 6 0 R /Loop 8 0 R >> >> >>".to_string(),
            stream("", page),
            "<< /Type /StructTreeRoot /K [9 0 R 10 0 R] >>".to_string(),
            stream("/Type /XObject /Subtype /Form /Resources << /Font << /F1 7 0 R >> >>", form),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
            stream("/Type /XObject /Subtype /Form /Resources << /Font << /F1 7 0 R >> /XObject << /Loop 8 0 R >> >>", looping),
            "<< /Type /StructElem /S /P /P 5 0 R /Pg 3 0 R /K 0 >>".to_string(),
            "<< /Type /StructElem /S /P /P 5 0 R /Pg 3 0 R /K 1 >>".to_string(),
        ]);
        let order = read_in_order(&doc).expect("tagged");
        let texts: Vec<&str> = order.blocks.iter().map(|b| b.text.as_str()).collect();
        assert_eq!(texts[0], "Inside the form");
        // The looping form is read down to the depth limit and then stops.
        assert!(texts[1].starts_with("Once"), "{texts:?}");
        assert!(texts[1].matches("Once").count() <= MAX_FORM_DEPTH + 1, "{texts:?}");
    }
}
