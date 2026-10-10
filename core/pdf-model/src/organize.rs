//! Page organization commands: reorder, rotate, delete, insert. [FR-ORG, M3]
//!
//! These are the simplest real edits to exercise the CoW overlay + command
//! journal pipeline end-to-end. Each operation is expressed as a set of
//! SetObjectCommands that modify the Pages tree. [ADR-012, SDS §3.4]

use crate::command::CommandGroup;

/// Set a page's `/Rotate` to `rotation`, preserving every other key.
///
/// Rotation used to be applied by writing `N 0 obj << /Rotate D >> endobj`
/// over the page object, replacing /Type, /Parent, /MediaBox, /Contents and
/// /Resources with nothing at all: a rotated page would have lost its
/// content. It never fired only because the caller passed page object numbers
/// where the builder matched page indices, so the rotation applied to no page
/// and silently did nothing. [FR-ROTATE, ADR-012, PRIN-1, GR-8]
pub fn set_page_rotation(page_bytes: &[u8], rotation: u32) -> Result<Vec<u8>, String> {
    let text = String::from_utf8_lossy(page_bytes).to_string();
    let dict_end = text
        .rfind(">>")
        .ok_or_else(|| "page object has no dictionary end".to_string())?;

    if let Some(at) = text.find("/Rotate") {
        let after = at + "/Rotate".len();
        let rest = &text[after..];
        let value_start = after
            + rest
                .find(|c: char| !c.is_ascii_whitespace())
                .ok_or_else(|| "/Rotate has no value".to_string())?;
        let value_end = value_start
            + text[value_start..]
                .find(|c: char| !(c.is_ascii_digit() || c == '-' || c == '+'))
                .ok_or_else(|| "/Rotate value never ends".to_string())?;
        let mut patched = String::with_capacity(text.len() + 8);
        patched.push_str(&text[..at]);
        patched.push_str(&format!("/Rotate {rotation}"));
        patched.push_str(&text[value_end..]);
        return Ok(patched.into_bytes());
    }

    let mut patched = String::with_capacity(text.len() + 16);
    patched.push_str(&text[..dict_end]);
    patched.push_str(&format!("/Rotate {rotation} "));
    patched.push_str(&text[dict_end..]);
    Ok(patched.into_bytes())
}

/// Read a page's current `/Rotate`, defaulting to 0.
#[must_use]
pub fn page_rotation(page_bytes: &[u8]) -> u32 {
    let text = String::from_utf8_lossy(page_bytes);
    let Some(at) = text.find("/Rotate") else {
        return 0;
    };
    text[at + "/Rotate".len()..]
        .trim_start()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .unwrap_or(0)
}

/// The raw value of `/Key` in an object, if present: a token such as `90`, or
/// an array such as `[0 0 612 792]`. The key must be a whole name, so `/Rotate`
/// does not match `/RotateBy`. An indirect value (`N G R`) is returned as
/// written; callers that need a number treat it as unsupported.
fn entry_value(bytes: &[u8], key: &str) -> Option<String> {
    let key = key.as_bytes();
    let mut from = 0;
    while let Some(found) = bytes[from..].windows(key.len()).position(|w| w == key) {
        let at = from + found;
        let after = at + key.len();
        let whole_name = bytes
            .get(after)
            .is_none_or(|b| b.is_ascii_whitespace() || matches!(b, b'[' | b'<' | b'(' | b'/' | b'>'));
        if whole_name {
            let mut i = after;
            while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
                i += 1;
            }
            if bytes.get(i) == Some(&b'[') {
                let close = bytes[i..].iter().position(|&b| b == b']')? + i;
                return Some(String::from_utf8_lossy(&bytes[i..=close]).into_owned());
            }
            let end = bytes[i..]
                .iter()
                .position(|b| matches!(b, b'/' | b'>' | b'[' | b'<'))
                .map_or(bytes.len(), |p| p + i);
            let value = String::from_utf8_lossy(&bytes[i..end]).trim().to_string();
            return (!value.is_empty()).then_some(value);
        }
        from = after;
    }
    None
}

/// The value `leaf` has for an inheritable key: its own, else the nearest
/// ancestor's. `/MediaBox`, `/CropBox`, `/Rotate` and `/Resources` are inherited
/// through `/Pages` nodes, which is why producers put them there. [FR-ORG-1]
fn inherited_value(nodes: &[PageTreeNode], leaf: u32, key: &str) -> Option<String> {
    let by_number: std::collections::HashMap<u32, &PageTreeNode> =
        nodes.iter().map(|n| (n.obj_num, n)).collect();
    let mut parent_of = std::collections::HashMap::new();
    for node in nodes {
        for kid in node.kids.iter().flatten() {
            parent_of.insert(*kid, node.obj_num);
        }
    }
    let mut current = Some(leaf);
    let mut hops = 0;
    while let Some(number) = current {
        if let Some(value) = by_number.get(&number).and_then(|n| entry_value(&n.bytes, key)) {
            return Some(value);
        }
        hops += 1;
        if hops > 64 {
            return None;
        }
        current = parent_of.get(&number).copied();
    }
    None
}

/// The rotation a page actually has: its own `/Rotate`, else an ancestor's,
/// else 0, normalised to 0, 90, 180 or 270. [FR-ROTATE]
#[must_use]
pub fn effective_rotation(nodes: &[PageTreeNode], leaf: u32) -> u32 {
    inherited_value(nodes, leaf, "/Rotate")
        .and_then(|v| v.parse::<i64>().ok())
        .map_or(0, |degrees| degrees.rem_euclid(360) as u32)
}

/// The `/MediaBox` a page actually has, as `[llx, lly, urx, ury]`.
pub fn effective_media_box(nodes: &[PageTreeNode], leaf: u32) -> Result<[f64; 4], String> {
    let raw = inherited_value(nodes, leaf, "/MediaBox")
        .ok_or_else(|| "the page has no /MediaBox of its own or from an ancestor".to_string())?;
    parse_rect(&raw).map_err(|why| format!("the /MediaBox {raw:?} is not usable: {why}"))
}

/// `[a b c d]` as four finite numbers.
fn parse_rect(raw: &str) -> Result<[f64; 4], String> {
    let inner = raw.trim().strip_prefix('[').and_then(|s| s.strip_suffix(']')).ok_or("not an array")?;
    let numbers: Vec<f64> = inner
        .split_whitespace()
        .map(|t| t.parse::<f64>().map_err(|_| format!("'{t}' is not a number")))
        .collect::<Result<_, _>>()?;
    let [a, b, c, d] = numbers[..] else {
        return Err(format!("{} numbers instead of 4", numbers.len()));
    };
    if ![a, b, c, d].iter().all(|n| n.is_finite()) {
        return Err("a value is not finite".into());
    }
    Ok([a, b, c, d])
}

/// One node of a document's page tree, as read from the file. [FR-ORG-1]
#[derive(Debug, Clone)]
pub struct PageTreeNode {
    /// Object number.
    pub obj_num: u32,
    /// The whole object, `N G obj ... endobj`.
    pub bytes: Vec<u8>,
    /// The kids of a `/Pages` node; `None` for a page.
    pub kids: Option<Vec<u32>>,
}

/// Where the `/Kids [ ... ]` array sits in a node: the byte range between the
/// brackets. `Ok(None)` for an object with no `/Kids`, which is a page.
fn kids_span(bytes: &[u8]) -> Result<Option<(usize, usize)>, String> {
    const KEY: &[u8] = b"/Kids";
    let Some(at) = bytes.windows(KEY.len()).position(|w| w == KEY) else {
        return Ok(None);
    };
    let mut i = at + KEY.len();
    while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
        i += 1;
    }
    match bytes.get(i) {
        Some(b'[') => {
            let open = i + 1;
            let close = bytes[open..]
                .iter()
                .position(|&b| b == b']')
                .ok_or_else(|| "the /Kids array is never closed".to_string())?
                + open;
            Ok(Some((open, close)))
        }
        Some(b) if b.is_ascii_digit() => {
            Err("an indirect /Kids array is not supported".to_string())
        }
        _ => Err("the /Kids entry is not an array".to_string()),
    }
}

/// The object numbers a `/Pages` node lists as kids, in order. `Ok(None)` for a
/// page. A reference with a non-zero generation, or any token that is not part
/// of an `N G R` triple, is an error: rewriting such an array would change what
/// it refers to. [FR-ORG-1, GR-8]
pub fn kid_references(node_bytes: &[u8]) -> Result<Option<Vec<u32>>, String> {
    let Some((open, close)) = kids_span(node_bytes)? else {
        return Ok(None);
    };
    let text = String::from_utf8_lossy(&node_bytes[open..close]);
    let tokens: Vec<&str> = text.split_whitespace().collect();
    if tokens.len() % 3 != 0 {
        return Err("the /Kids array holds something other than references".to_string());
    }
    let mut kids = Vec::with_capacity(tokens.len() / 3);
    for triple in tokens.chunks(3) {
        let (Ok(number), Ok(generation), "R") = (triple[0].parse::<u32>(), triple[1].parse::<u32>(), triple[2])
        else {
            return Err("the /Kids array holds something other than references".to_string());
        };
        if generation != 0 {
            return Err(format!("kid {number} has generation {generation}; only generation 0 is supported"));
        }
        kids.push(number);
    }
    Ok(Some(kids))
}

/// The page objects of a tree in document order. A tree that loops or nests
/// deeper than 64 levels is an error rather than a hang. [GR-7, GR-8]
pub fn leaf_pages(nodes: &[PageTreeNode]) -> Result<Vec<u32>, String> {
    const MAX_DEPTH: usize = 64;
    let root = nodes.first().ok_or_else(|| "the page tree is empty".to_string())?;
    let by_number: std::collections::HashMap<u32, &PageTreeNode> =
        nodes.iter().map(|n| (n.obj_num, n)).collect();

    fn walk(
        node: &PageTreeNode,
        by_number: &std::collections::HashMap<u32, &PageTreeNode>,
        depth: usize,
        seen: &mut std::collections::HashSet<u32>,
        out: &mut Vec<u32>,
    ) -> Result<(), String> {
        if depth > MAX_DEPTH {
            return Err("the page tree is nested too deeply".to_string());
        }
        if !seen.insert(node.obj_num) {
            return Err(format!("the page tree visits object {} twice", node.obj_num));
        }
        match &node.kids {
            None => out.push(node.obj_num),
            Some(kids) => {
                for kid in kids {
                    let child = by_number
                        .get(kid)
                        .ok_or_else(|| format!("kid {kid} is not in the page tree that was read"))?;
                    walk(child, by_number, depth + 1, seen, out)?;
                }
            }
        }
        Ok(())
    }

    let mut out = Vec::new();
    walk(root, &by_number, 0, &mut std::collections::HashSet::new(), &mut out)?;
    Ok(out)
}

/// Replace a `/Pages` node's kids and `/Count`, leaving every other key alone.
///
/// The previous builder rewrote the whole object as
/// `<< /Type /Pages /Kids [..] /Count n >>`, which dropped `/MediaBox`,
/// `/Resources`, `/Rotate` and `/Parent`: attributes that producers put on the
/// node precisely so the pages inherit them. Deleting one page could change the
/// size of every other. [FR-ORG-1, PRIN-1, GR-8]
fn set_kids_and_count(bytes: &[u8], kids: &[u32], count: u32) -> Result<Vec<u8>, String> {
    let (open, close) = kids_span(bytes)?.ok_or_else(|| "the node has no /Kids".to_string())?;
    let list = kids.iter().map(|k| format!("{k} 0 R")).collect::<Vec<_>>().join(" ");

    // The first /Count outside the kids array.
    const KEY: &[u8] = b"/Count";
    let count_at = bytes
        .windows(KEY.len())
        .enumerate()
        .find(|(i, w)| *w == KEY && !(open..close).contains(i))
        .map(|(i, _)| i);

    let mut out = Vec::with_capacity(bytes.len() + list.len());
    let mut cursor = 0;
    let mut edits: Vec<(usize, usize, Vec<u8>)> = vec![(open, close, list.into_bytes())];
    match count_at {
        Some(at) => {
            let mut i = at + KEY.len();
            while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
                i += 1;
            }
            let start = i;
            while bytes.get(i).is_some_and(|b| b.is_ascii_digit() || *b == b'-') {
                i += 1;
            }
            if start == i {
                return Err("the /Count has no value".to_string());
            }
            edits.push((start, i, count.to_string().into_bytes()));
        }
        None => {
            let end = bytes
                .windows(2)
                .rposition(|w| w == b">>")
                .ok_or_else(|| "the node has no dictionary end".to_string())?;
            edits.push((end, end, format!(" /Count {count} ").into_bytes()));
        }
    }
    edits.sort_by_key(|(from, _, _)| *from);
    for (from, to, replacement) in edits {
        out.extend_from_slice(&bytes[cursor..from]);
        out.extend_from_slice(&replacement);
        cursor = to;
    }
    out.extend_from_slice(&bytes[cursor..]);
    Ok(out)
}

/// Delete pages by 0-based index from a page tree of any depth.
///
/// `nodes` is the whole tree, root first. Only the nodes that lose a kid, and
/// the ancestors whose `/Count` changes, are rewritten, in place. An emptied
/// intermediate node stays, with no kids and a count of zero: legal, and not
/// worth restructuring the tree for. The deleted page objects are left in the
/// file, which is what lets undo restore them. Deleting every page is refused.
/// [FR-ORG-1, ADR-012, PRIN-1, GR-8]
pub fn build_delete_pages_group(
    nodes: &[PageTreeNode],
    page_indices: &[u32],
) -> Result<CommandGroup, String> {
    if page_indices.is_empty() {
        return Err("no pages to delete".into());
    }
    let leaves = leaf_pages(nodes)?;
    let mut doomed = std::collections::HashSet::new();
    for &index in page_indices {
        let leaf = leaves.get(index as usize).ok_or_else(|| {
            format!("page {} is not in the document, which has {} page(s)", index + 1, leaves.len())
        })?;
        doomed.insert(*leaf);
    }
    if doomed.len() == leaves.len() {
        return Err("that would delete every page; a document needs at least one".into());
    }

    let by_number: std::collections::HashMap<u32, &PageTreeNode> =
        nodes.iter().map(|n| (n.obj_num, n)).collect();
    // (pages in the subtree before, pages in it after)
    fn counts(
        node: &PageTreeNode,
        by_number: &std::collections::HashMap<u32, &PageTreeNode>,
        doomed: &std::collections::HashSet<u32>,
    ) -> (u32, u32) {
        match &node.kids {
            None => (1, u32::from(!doomed.contains(&node.obj_num))),
            Some(kids) => kids.iter().filter_map(|k| by_number.get(k)).fold((0, 0), |acc, child| {
                let (before, after) = counts(child, by_number, doomed);
                (acc.0 + before, acc.1 + after)
            }),
        }
    }

    let mut group = CommandGroup::new(format!("Delete {} page(s)", doomed.len()));
    for node in nodes {
        let Some(kids) = &node.kids else { continue };
        let (before, after) = counts(node, &by_number, &doomed);
        if before == after {
            continue;
        }
        let surviving: Vec<u32> = kids.iter().copied().filter(|k| !doomed.contains(k)).collect();
        let new_bytes = set_kids_and_count(&node.bytes, &surviving, after)?;
        group.push(Box::new(crate::command::SetObjectCommand {
            obj_num: node.obj_num,
            new_bytes,
            old_bytes: Some(node.bytes.clone()),
        }));
    }
    Ok(group)
}

/// Parents and subtree sizes for a page tree. [FR-ORG-1]
struct TreeIndex<'a> {
    by_number: std::collections::HashMap<u32, &'a PageTreeNode>,
    parent: std::collections::HashMap<u32, u32>,
}

impl<'a> TreeIndex<'a> {
    fn new(nodes: &'a [PageTreeNode]) -> Self {
        let mut parent = std::collections::HashMap::new();
        for node in nodes {
            for kid in node.kids.iter().flatten() {
                parent.insert(*kid, node.obj_num);
            }
        }
        Self { by_number: nodes.iter().map(|n| (n.obj_num, n)).collect(), parent }
    }

    /// Ancestors of `node`, nearest first.
    fn ancestors(&self, node: u32) -> Vec<u32> {
        let mut out = Vec::new();
        let mut current = node;
        while let Some(&p) = self.parent.get(&current) {
            out.push(p);
            current = p;
            if out.len() > 64 {
                break;
            }
        }
        out
    }

    /// Pages at or below `node`.
    fn leaves_under(&self, node: u32) -> u32 {
        match self.by_number.get(&node).and_then(|n| n.kids.as_ref()) {
            None => 1,
            Some(kids) => kids.iter().map(|k| self.leaves_under(*k)).sum(),
        }
    }
}

/// Edits to a set of `/Pages` nodes: their new kids, and how many pages each
/// gains. Applied once per node, so several changes under one parent become one
/// rewrite.
#[derive(Default)]
struct TreeEdits {
    kids: std::collections::HashMap<u32, Vec<u32>>,
    delta: std::collections::HashMap<u32, i64>,
}

impl TreeEdits {
    fn finish(self, nodes: &[PageTreeNode], group: &mut CommandGroup) -> Result<(), String> {
        let index = TreeIndex::new(nodes);
        for node in nodes {
            let touched = self.kids.contains_key(&node.obj_num) || self.delta.contains_key(&node.obj_num);
            if !touched {
                continue;
            }
            let kids = self
                .kids
                .get(&node.obj_num)
                .cloned()
                .or_else(|| node.kids.clone())
                .ok_or_else(|| format!("object {} is a page, not a /Pages node", node.obj_num))?;
            let count = i64::from(index.leaves_under(node.obj_num)) + self.delta.get(&node.obj_num).copied().unwrap_or(0);
            let count = u32::try_from(count).map_err(|_| "the page count would go negative".to_string())?;
            group.push(Box::new(crate::command::SetObjectCommand {
                obj_num: node.obj_num,
                new_bytes: set_kids_and_count(&node.bytes, &kids, count)?,
                old_bytes: Some(node.bytes.clone()),
            }));
        }
        Ok(())
    }
}

/// Sorted, distinct, in-range page indices as page object numbers, in document
/// order.
fn chosen_pages(leaves: &[u32], page_indices: &[u32]) -> Result<Vec<u32>, String> {
    if page_indices.is_empty() {
        return Err("no pages chosen".into());
    }
    let mut sorted: Vec<u32> = page_indices.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    sorted
        .iter()
        .map(|&i| {
            leaves.get(i as usize).copied().ok_or_else(|| {
                format!("page {} is not in the document, which has {} page(s)", i + 1, leaves.len())
            })
        })
        .collect()
}

/// Move pages so the first of them lands at zero-based position `to` of the
/// resulting document; the moved pages keep their relative order.
///
/// Only a move that is a reordering of one `/Pages` node's `/Kids` is supported:
/// every moved page must share a parent, and the result must be reachable by
/// reordering that node alone. A page that changes parent would also change what
/// it inherits (`/MediaBox`, `/Rotate`, `/Resources`), and is refused by name
/// rather than silently altering how the page looks. A flat tree, which is most
/// documents, is always fine. [FR-ORG-1, PRIN-1, GR-8]
pub fn build_move_pages_group(
    nodes: &[PageTreeNode],
    page_indices: &[u32],
    to: u32,
) -> Result<CommandGroup, String> {
    let leaves = leaf_pages(nodes)?;
    let moved = chosen_pages(&leaves, page_indices)?;
    let remaining: Vec<u32> = leaves.iter().copied().filter(|l| !moved.contains(l)).collect();
    if to as usize > remaining.len() {
        return Err(format!(
            "position {} is past the end: after taking {} page(s) out, {} remain",
            to + 1,
            moved.len(),
            remaining.len()
        ));
    }
    let mut desired = remaining[..to as usize].to_vec();
    desired.extend(&moved);
    desired.extend(&remaining[to as usize..]);
    if desired == leaves {
        return Err("the pages are already there".into());
    }

    let index = TreeIndex::new(nodes);
    let parent = *index.parent.get(&moved[0]).ok_or("the page has no parent in the tree")?;
    if moved.iter().any(|m| index.parent.get(m) != Some(&parent)) {
        return Err("the chosen pages belong to different branches of the page tree; \
                    moving between branches is not supported"
            .into());
    }

    let node = index.by_number[&parent];
    let kids = node.kids.clone().unwrap_or_default();
    let remaining_kids: Vec<u32> = kids.iter().copied().filter(|k| !moved.contains(k)).collect();
    let after = to as usize + moved.len();
    let position = match desired.get(after) {
        Some(&anchor) => {
            // The kid of this node that holds the page the moved ones must precede.
            let mut holder = anchor;
            while index.parent.get(&holder) != Some(&parent) {
                holder = *index.parent.get(&holder).ok_or_else(|| {
                    "that move crosses into another branch of the page tree, which is not supported".to_string()
                })?;
            }
            remaining_kids
                .iter()
                .position(|k| *k == holder)
                .ok_or_else(|| "that move crosses into another branch of the page tree".to_string())?
        }
        None => remaining_kids.len(),
    };
    let mut new_kids = remaining_kids[..position].to_vec();
    new_kids.extend(&moved);
    new_kids.extend(&remaining_kids[position..]);

    // The order the whole document would have: it must be the one asked for.
    let mut check = nodes.to_vec();
    if let Some(n) = check.iter_mut().find(|n| n.obj_num == parent) {
        n.kids = Some(new_kids.clone());
    }
    if leaf_pages(&check)? != desired {
        return Err("that move crosses into another branch of the page tree, which is not supported".into());
    }

    let mut group = CommandGroup::new(format!("Move {} page(s)", moved.len()));
    let mut edits = TreeEdits::default();
    edits.kids.insert(parent, new_kids);
    edits.finish(nodes, &mut group)?;
    Ok(group)
}

/// The same object with a new number: `N G obj ...` becomes `M 0 obj ...`.
fn renumbered(bytes: &[u8], new_number: u32) -> Result<Vec<u8>, String> {
    let at = bytes
        .windows(3)
        .position(|w| w == b"obj")
        .ok_or_else(|| "the object has no obj keyword".to_string())?;
    let mut out = format!("{new_number} 0 ").into_bytes();
    out.extend_from_slice(&bytes[at..]);
    Ok(out)
}

/// Duplicate pages: each copy is a new page object right after its original, in
/// the same parent, sharing the original's content and resources by reference.
///
/// A page that has annotations is refused: the copy would share the original's
/// annotation objects, and a form field or note cannot belong to two pages.
/// Returns the group and the next free object number. [FR-ORG-1, PRIN-1, GR-8]
pub fn build_duplicate_pages_group(
    nodes: &[PageTreeNode],
    page_indices: &[u32],
    next_obj_num: u32,
) -> Result<(CommandGroup, u32), String> {
    let leaves = leaf_pages(nodes)?;
    let chosen = chosen_pages(&leaves, page_indices)?;
    let index = TreeIndex::new(nodes);

    let mut group = CommandGroup::new(format!("Duplicate {} page(s)", chosen.len()));
    let mut edits = TreeEdits::default();
    let mut copies: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
    let mut next = next_obj_num;
    for &leaf in &chosen {
        let node = index.by_number[&leaf];
        if node.bytes.windows(b"/Annots".len()).any(|w| w == b"/Annots") {
            return Err(format!(
                "page object {leaf} has annotations; duplicating a page that has annotations \
                 is not supported, because the copy would share them"
            ));
        }
        copies.insert(leaf, next);
        group.push(Box::new(crate::command::SetObjectCommand {
            obj_num: next,
            new_bytes: renumbered(&node.bytes, next)?,
            old_bytes: None,
        }));
        next += 1;
        for ancestor in index.ancestors(leaf) {
            *edits.delta.entry(ancestor).or_insert(0) += 1;
        }
    }
    for (&leaf, &copy) in &copies {
        let parent = *index.parent.get(&leaf).ok_or("the page has no parent in the tree")?;
        let current = edits.kids.entry(parent).or_insert_with(|| index.by_number[&parent].kids.clone().unwrap_or_default());
        let at = current.iter().position(|k| *k == leaf).ok_or("the page is not among its parent's kids")?;
        current.insert(at + 1, copy);
    }
    edits.finish(nodes, &mut group)?;
    Ok((group, next))
}

/// Insert one blank page so it becomes zero-based page `at` of the result.
///
/// It joins the parent of the page it sits next to, and takes that page's media
/// box unless `size` (width, height in points) is given. [FR-ORG-1]
pub fn build_insert_blank_group(
    nodes: &[PageTreeNode],
    at: u32,
    size: Option<[f64; 2]>,
    next_obj_num: u32,
) -> Result<(CommandGroup, u32), String> {
    let leaves = leaf_pages(nodes)?;
    if at as usize > leaves.len() {
        return Err(format!("position {} is past the end: the document has {} page(s)", at + 1, leaves.len()));
    }
    let index = TreeIndex::new(nodes);
    let (neighbour, before) = match leaves.get(at as usize) {
        Some(&l) => (l, true),
        None => (*leaves.last().ok_or("the document has no pages")?, false),
    };
    let parent = *index.parent.get(&neighbour).ok_or("the page has no parent in the tree")?;
    let [width, height] = match size {
        Some(s) => s,
        None => {
            let b = effective_media_box(nodes, neighbour).unwrap_or([0.0, 0.0, 612.0, 792.0]);
            [(b[2] - b[0]).abs(), (b[3] - b[1]).abs()]
        }
    };
    if !(width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0) {
        return Err(format!("a page of {width} x {height} points is not a page"));
    }

    let new_bytes = format!(
        "{next_obj_num} 0 obj\n<< /Type /Page /Parent {parent} 0 R /MediaBox [0 0 {} {}] /Resources << >> >>\nendobj\n",
        pdf_number(width),
        pdf_number(height)
    )
    .into_bytes();

    let mut group = CommandGroup::new("Insert a blank page".to_string());
    group.push(Box::new(crate::command::SetObjectCommand {
        obj_num: next_obj_num,
        new_bytes,
        old_bytes: None,
    }));
    let mut kids = index.by_number[&parent].kids.clone().unwrap_or_default();
    let place = kids.iter().position(|k| *k == neighbour).ok_or("the page is not among its parent's kids")?;
    kids.insert(if before { place } else { place + 1 }, next_obj_num);
    let mut edits = TreeEdits::default();
    edits.kids.insert(parent, kids);
    for ancestor in std::iter::once(parent).chain(index.ancestors(parent)) {
        *edits.delta.entry(ancestor).or_insert(0) += 1;
    }
    edits.finish(nodes, &mut group)?;
    Ok((group, next_obj_num + 1))
}
/// Build a rotate-pages command group, patching each page in place.
///
/// `pages` is `(object number, current page bytes, rotation the page has now)`
/// for every page to rotate. The rotation the page has is its own `/Rotate` or
/// the one it inherits (see [`effective_rotation`]): adding to the page's own
/// value alone turned a page that inherited 90 into a page rotated by the new
/// amount, not by 90 plus it. The bytes are required for two reasons: rotation
/// must preserve the rest of the page dictionary, and undo must restore exactly
/// what was there.
///
/// An earlier signature took page *indices* alongside a list the caller
/// populated with page *object numbers*, and matched one against the other, so
/// the lookup never succeeded and rotation silently did nothing. Taking the
/// pages themselves removes the mismatch rather than documenting it.
/// [FR-ROTATE, ADR-012, PRIN-1]
pub fn build_rotate_pages_group(
    pages: &[(u32, Vec<u8>, u32)],
    degrees: u32,
) -> Result<CommandGroup, String> {
    if pages.is_empty() {
        return Err("no pages to rotate".into());
    }
    if !matches!(degrees, 90 | 180 | 270) {
        return Err(format!("invalid rotation: {degrees} (must be 90, 180, or 270)"));
    }

    let mut group = CommandGroup::new(format!("Rotate {} page(s) by {degrees}°", pages.len()));
    for (obj_num, page_bytes, current) in pages {
        let rotation = (current + degrees) % 360;
        let new_bytes = set_page_rotation(page_bytes, rotation)?;
        group.push(Box::new(crate::command::SetObjectCommand {
            obj_num: *obj_num,
            new_bytes,
            old_bytes: Some(page_bytes.clone()),
        }));
    }
    Ok(group)
}

/// A number as PDF writes it: no exponent, no trailing zeros.
fn pdf_number(n: f64) -> String {
    let text = format!("{n:.4}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Set a page's `/CropBox`, replacing an existing one or adding it, and leave
/// every other key alone. [FR-ORG, PRIN-1]
fn set_page_crop_box(page_bytes: &[u8], rect: [f64; 4]) -> Result<Vec<u8>, String> {
    let array = format!("[{} {} {} {}]", pdf_number(rect[0]), pdf_number(rect[1]), pdf_number(rect[2]), pdf_number(rect[3]));
    const KEY: &[u8] = b"/CropBox";
    if let Some(at) = page_bytes.windows(KEY.len()).position(|w| w == KEY) {
        let mut i = at + KEY.len();
        while page_bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
            i += 1;
        }
        if page_bytes.get(i) != Some(&b'[') {
            return Err("the page's /CropBox is not an inline array".to_string());
        }
        let close = page_bytes[i..]
            .iter()
            .position(|&b| b == b']')
            .ok_or_else(|| "the page's /CropBox is never closed".to_string())?
            + i;
        let mut out = Vec::with_capacity(page_bytes.len() + 16);
        out.extend_from_slice(&page_bytes[..at]);
        out.extend_from_slice(format!("/CropBox {array}").as_bytes());
        out.extend_from_slice(&page_bytes[close + 1..]);
        return Ok(out);
    }
    let end = page_bytes
        .windows(2)
        .rposition(|w| w == b">>")
        .ok_or_else(|| "page object has no dictionary end".to_string())?;
    let mut out = Vec::with_capacity(page_bytes.len() + 32);
    out.extend_from_slice(&page_bytes[..end]);
    out.extend_from_slice(format!(" /CropBox {array} ").as_bytes());
    out.extend_from_slice(&page_bytes[end..]);
    Ok(out)
}

/// Build a crop command group.
///
/// `pages` is `(object number, current bytes, the page's effective /MediaBox)`.
/// The box must be a real rectangle (`llx < urx`, `lly < ury`) and lie within
/// every page's `/MediaBox`: a crop box outside the media box is clipped by
/// readers to something other than what the user typed, and the command should
/// not claim to have done what it did not. [FR-ORG, GR-8, UX-ERR-3]
pub fn build_crop_pages_group(
    pages: &[(u32, Vec<u8>, [f64; 4])],
    rect: [f64; 4],
) -> Result<CommandGroup, String> {
    if pages.is_empty() {
        return Err("no pages to crop".into());
    }
    let [x0, y0, x1, y1] = rect;
    if ![x0, y0, x1, y1].iter().all(|n| n.is_finite()) || x1 <= x0 || y1 <= y0 {
        return Err(format!(
            "the crop box [{x0} {y0} {x1} {y1}] is not a rectangle: it needs llx < urx and lly < ury"
        ));
    }
    let mut group = CommandGroup::new(format!("Crop {} page(s)", pages.len()));
    for (obj_num, page_bytes, media) in pages {
        // Media boxes may be written with their corners in either order.
        let (m_x0, m_x1) = (media[0].min(media[2]), media[0].max(media[2]));
        let (m_y0, m_y1) = (media[1].min(media[3]), media[1].max(media[3]));
        if x0 < m_x0 || y0 < m_y0 || x1 > m_x1 || y1 > m_y1 {
            return Err(format!(
                "the crop box [{x0} {y0} {x1} {y1}] extends past page object {obj_num}'s media box \
                 [{m_x0} {m_y0} {m_x1} {m_y1}]"
            ));
        }
        group.push(Box::new(crate::command::SetObjectCommand {
            obj_num: *obj_num,
            new_bytes: set_page_crop_box(page_bytes, rect)?,
            old_bytes: Some(page_bytes.clone()),
        }));
    }
    Ok(group)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::overlay::CowOverlay;

    fn node(obj_num: u32, dict: &str) -> PageTreeNode {
        let bytes = format!("{obj_num} 0 obj\n{dict}\nendobj\n").into_bytes();
        let kids = kid_references(&bytes).expect("kids");
        PageTreeNode { obj_num, bytes, kids }
    }

    fn page(obj_num: u32, parent: u32) -> PageTreeNode {
        node(obj_num, &format!("<< /Type /Page /Parent {parent} 0 R /MediaBox [0 0 612 792] >>"))
    }

    /// root(2) -> A(3) -> pages 5, 6 ; B(4) -> pages 7, 8
    fn nested() -> Vec<PageTreeNode> {
        vec![
            node(2, "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 4 /MediaBox [0 0 300 400] >>"),
            node(3, "<< /Type /Pages /Parent 2 0 R /Kids [5 0 R 6 0 R] /Count 2 >>"),
            page(5, 3),
            page(6, 3),
            node(4, "<< /Type /Pages /Parent 2 0 R /Kids [7 0 R 8 0 R] /Count 2 >>"),
            page(7, 4),
            page(8, 4),
        ]
    }

    fn text(overlay: &CowOverlay, n: u32) -> String {
        String::from_utf8_lossy(overlay.get_object(n).expect("object")).to_string()
    }

    #[test]
    fn kid_references_reads_references_and_refuses_what_it_cannot_rewrite() {
        let flat = b"2 0 obj\n<< /Type /Pages /Kids [3 0 R 4 0 R\n5 0 R] /Count 3 >>\nendobj\n";
        assert_eq!(kid_references(flat).unwrap(), Some(vec![3, 4, 5]), "spaces and newlines");
        assert_eq!(kid_references(b"<< /Type /Page >>").unwrap(), None, "a page has no kids");
        assert_eq!(kid_references(b"<< /Kids [] /Count 0 >>").unwrap(), Some(vec![]));
        assert!(kid_references(b"<< /Kids 9 0 R >>").unwrap_err().contains("indirect"));
        assert!(kid_references(b"<< /Kids [3 1 R] >>").unwrap_err().contains("generation"));
        assert!(kid_references(b"<< /Kids [3 0 R 4] >>").is_err(), "a dangling token");
        assert!(kid_references(b"<< /Kids [3 0 R").is_err(), "an unclosed array");
    }

    #[test]
    fn set_kids_and_count_keeps_every_other_key() {
        let original = b"2 0 obj\n<< /Type /Pages /Parent 1 0 R /MediaBox [0 0 300 400] \
/Resources << /Font << /F1 9 0 R >> >> /Kids [3 0 R 4 0 R] /Count 2 /Rotate 90 >>\nendobj\n";
        let out = String::from_utf8(set_kids_and_count(original, &[4], 1).unwrap()).unwrap();
        assert!(out.contains("/Kids [4 0 R]") && out.contains("/Count 1"), "{out}");
        for kept in ["/MediaBox [0 0 300 400]", "/Resources << /Font << /F1 9 0 R >> >>", "/Rotate 90", "/Parent 1 0 R"] {
            assert!(out.contains(kept), "lost {kept}: {out}");
        }
        assert!(out.starts_with("2 0 obj\n") && out.ends_with("endobj\n"));
    }

    #[test]
    fn a_count_inside_the_kids_region_is_not_mistaken_for_the_nodes_count() {
        // /Count is only ever read outside the array, and a node without one
        // gets one rather than a corrupt edit.
        let bare = b"2 0 obj\n<< /Type /Pages /Kids [3 0 R] >>\nendobj\n";
        let out = String::from_utf8(set_kids_and_count(bare, &[3], 1).unwrap()).unwrap();
        assert!(out.contains("/Count 1"), "{out}");
    }

    #[test]
    fn deleting_a_page_keeps_what_the_pages_node_hands_down() {
        let nodes = vec![
            node(2, "<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 /MediaBox [0 0 300 400] /Resources << >> >>"),
            page(3, 2),
            page(4, 2),
            page(5, 2),
        ];
        let group = build_delete_pages_group(&nodes, &[1]).unwrap();
        assert_eq!(group.name, "Delete 1 page(s)");
        let mut overlay = CowOverlay::new();
        group.apply(&mut overlay).unwrap();
        let root = text(&overlay, 2);
        assert!(root.contains("/Count 2") && root.contains("3 0 R") && root.contains("5 0 R"), "{root}");
        assert!(!root.contains("4 0 R"));
        assert!(root.contains("/MediaBox [0 0 300 400]"), "the inherited page size was dropped: {root}");
        assert!(root.contains("/Resources"), "the inherited resources were dropped: {root}");
    }

    #[test]
    fn deleting_from_a_nested_tree_edits_the_right_node_and_its_ancestors() {
        let nodes = nested();
        assert_eq!(leaf_pages(&nodes).unwrap(), [5, 6, 7, 8], "document order across branches");

        let group = build_delete_pages_group(&nodes, &[1]).unwrap();
        assert_eq!(group.len(), 2, "branch A and the root; branch B is untouched");
        let mut overlay = CowOverlay::new();
        group.apply(&mut overlay).unwrap();

        let a = text(&overlay, 3);
        assert!(a.contains("/Kids [5 0 R]") && a.contains("/Count 1"), "{a}");
        let root = text(&overlay, 2);
        assert!(root.contains("/Kids [3 0 R 4 0 R]"), "the branches stay: {root}");
        assert!(root.contains("/Count 3"), "the root counts pages, not kids: {root}");
        assert!(root.contains("/MediaBox [0 0 300 400]"));
        assert!(overlay.get_object(4).is_none(), "branch B must not be rewritten");
    }

    #[test]
    fn emptying_a_branch_leaves_it_empty_and_legal() {
        let group = build_delete_pages_group(&nested(), &[0, 1]).unwrap();
        let mut overlay = CowOverlay::new();
        group.apply(&mut overlay).unwrap();
        let a = text(&overlay, 3);
        assert!(a.contains("/Kids []") && a.contains("/Count 0"), "{a}");
        assert!(text(&overlay, 2).contains("/Count 2"));
    }

    #[test]
    fn undo_restores_every_node_byte_for_byte() {
        let nodes = nested();
        let originals: Vec<(u32, Vec<u8>)> = nodes.iter().map(|n| (n.obj_num, n.bytes.clone())).collect();
        let mut overlay = CowOverlay::new();
        let group = build_delete_pages_group(&nodes, &[0, 3]).unwrap();
        group.apply(&mut overlay).unwrap();
        group.undo(&mut overlay).unwrap();
        for (number, bytes) in originals {
            if let Some(now) = overlay.get_object(number) {
                assert_eq!(now, bytes.as_slice(), "object {number} not restored");
            }
        }
    }

    #[test]
    fn what_cannot_be_deleted_is_refused_by_name() {
        let nodes = nested();
        assert!(build_delete_pages_group(&nodes, &[]).is_err());
        let past = build_delete_pages_group(&nodes, &[9]).unwrap_err();
        assert!(past.contains("page 10") && past.contains("4 page"), "{past}");
        assert!(build_delete_pages_group(&nodes, &[0, 1, 2, 3]).unwrap_err().contains("every page"));
    }

    #[test]
    fn a_page_tree_that_loops_is_an_error_not_a_hang() {
        let looping = vec![
            node(2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
            node(3, "<< /Type /Pages /Kids [2 0 R] /Count 1 >>"),
        ];
        assert!(leaf_pages(&looping).unwrap_err().contains("twice"));
    }

    #[test]
    fn an_entry_is_found_by_whole_name_and_in_either_shape() {
        assert_eq!(entry_value(b"<< /Rotate 90 /X 1 >>", "/Rotate").as_deref(), Some("90"));
        assert_eq!(entry_value(b"<< /RotateBy 5 >>", "/Rotate"), None, "a longer name is a different key");
        assert_eq!(
            entry_value(b"<< /MediaBox [0 0 612 792] /Rotate 0 >>", "/MediaBox").as_deref(),
            Some("[0 0 612 792]")
        );
        assert_eq!(entry_value(b"<< /Type /Page >>", "/Rotate"), None);
    }

    #[test]
    fn a_page_inherits_rotation_and_size_from_the_nearest_ancestor_that_has_them() {
        let nodes = vec![
            node(2, "<< /Type /Pages /Kids [3 0 R] /Count 2 /MediaBox [0 0 300 400] /Rotate 90 >>"),
            node(3, "<< /Type /Pages /Parent 2 0 R /Kids [5 0 R 6 0 R] /Count 2 >>"),
            node(5, "<< /Type /Page /Parent 3 0 R >>"),
            node(6, "<< /Type /Page /Parent 3 0 R /Rotate 180 /MediaBox [0 0 100 200] >>"),
        ];
        assert_eq!(effective_rotation(&nodes, 5), 90, "inherited through two levels");
        assert_eq!(effective_rotation(&nodes, 6), 180, "its own value wins");
        assert_eq!(effective_media_box(&nodes, 5).unwrap(), [0.0, 0.0, 300.0, 400.0]);
        assert_eq!(effective_media_box(&nodes, 6).unwrap(), [0.0, 0.0, 100.0, 200.0]);
    }

    #[test]
    fn rotation_is_normalised_and_a_missing_or_broken_box_is_an_error() {
        let rot = |v: &str| {
            let n = vec![node(2, &format!("<< /Type /Pages /Kids [3 0 R] /Count 1 /Rotate {v} >>")), node(3, "<< /Type /Page /Parent 2 0 R >>")];
            effective_rotation(&n, 3)
        };
        assert_eq!((rot("-90"), rot("450"), rot("0")), (270, 90, 0));
        let bare = vec![node(2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"), node(3, "<< /Type /Page /Parent 2 0 R >>")];
        assert_eq!(effective_rotation(&bare, 3), 0);
        assert!(effective_media_box(&bare, 3).unwrap_err().contains("no /MediaBox"));
        let broken = vec![node(2, "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 612] >>"), node(3, "<< /Type /Page /Parent 2 0 R >>")];
        assert!(effective_media_box(&broken, 3).unwrap_err().contains("3 numbers instead of 4"));
    }

    #[test]
    fn rotating_adds_to_the_rotation_the_page_has_even_when_it_is_inherited() {
        // The page has no /Rotate of its own and inherits 90: a further quarter
        // turn is 180, not 90. [FR-ROTATE]
        let page = b"5 0 obj\n<< /Type /Page /Parent 3 0 R >>\nendobj\n".to_vec();
        let group = build_rotate_pages_group(&[(5, page, 90)], 90).unwrap();
        let mut overlay = CowOverlay::new();
        group.apply(&mut overlay).unwrap();
        assert!(text(&overlay, 5).contains("/Rotate 180"), "{}", text(&overlay, 5));
    }

    #[test]
    fn a_crop_box_replaces_or_adds_and_keeps_everything_else() {
        let with = b"6 0 obj\n<< /Type /Page /CropBox [0 0 50 50] /Parent 2 0 R >>\nendobj\n";
        let out = String::from_utf8(set_page_crop_box(with, [10.0, 20.5, 200.0, 300.0]).unwrap()).unwrap();
        assert!(out.contains("/CropBox [10 20.5 200 300]") && !out.contains("[0 0 50 50]"), "{out}");
        assert!(out.contains("/Parent 2 0 R") && out.starts_with("6 0 obj\n") && out.ends_with("endobj\n"));

        let without = b"6 0 obj\n<< /Type /Page /Parent 2 0 R >>\nendobj\n";
        let out = String::from_utf8(set_page_crop_box(without, [0.0, 0.0, 100.0, 100.0]).unwrap()).unwrap();
        assert!(out.contains("/CropBox [0 0 100 100]") && out.contains("/Type /Page"), "{out}");
    }

    #[test]
    fn a_crop_is_refused_unless_it_is_a_rectangle_inside_every_media_box() {
        let page = |n: u32| (n, format!("{n} 0 obj\n<< /Type /Page >>\nendobj\n").into_bytes(), [0.0, 0.0, 612.0, 792.0]);
        let ok = build_crop_pages_group(&[page(5), page(6)], [10.0, 10.0, 200.0, 300.0]).unwrap();
        assert_eq!(ok.len(), 2);
        for (rect, why) in [
            ([200.0, 10.0, 10.0, 300.0], "left of right"),
            ([10.0, 300.0, 200.0, 10.0], "bottom above top"),
            ([10.0, 10.0, 10.0, 300.0], "zero width"),
            ([f64::NAN, 0.0, 10.0, 10.0], "not a number"),
        ] {
            assert!(build_crop_pages_group(&[page(5)], rect).unwrap_err().contains("not a rectangle"), "{why}");
        }
        let outside = build_crop_pages_group(&[page(5)], [0.0, 0.0, 700.0, 300.0]).unwrap_err();
        assert!(outside.contains("past page object 5") && outside.contains("media box"), "{outside}");
        assert!(build_crop_pages_group(&[], [0.0, 0.0, 1.0, 1.0]).is_err());
    }

    #[test]
    fn a_crop_can_be_undone_exactly() {
        let page = b"5 0 obj\n<< /Type /Page /CropBox [1 2 3 4] >>\nendobj\n".to_vec();
        let group = build_crop_pages_group(&[(5, page.clone(), [0.0, 0.0, 612.0, 792.0])], [10.0, 10.0, 100.0, 100.0]).unwrap();
        let mut overlay = CowOverlay::new();
        group.apply(&mut overlay).unwrap();
        assert!(text(&overlay, 5).contains("[10 10 100 100]"));
        group.undo(&mut overlay).unwrap();
        assert_eq!(overlay.get_object(5).unwrap(), page.as_slice());
    }

    fn flat3() -> Vec<PageTreeNode> {
        vec![
            node(2, "<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 >>"),
            page(3, 2),
            page(4, 2),
            page(5, 2),
        ]
    }

    fn kids_of(overlay: &CowOverlay, n: u32) -> Vec<u32> {
        kid_references(overlay.get_object(n).expect("object")).unwrap().unwrap()
    }

    #[test]
    fn a_page_moves_to_the_front_or_back_of_a_flat_tree() {
        let mut overlay = CowOverlay::new();
        build_move_pages_group(&flat3(), &[2], 0).unwrap().apply(&mut overlay).unwrap();
        assert_eq!(kids_of(&overlay, 2), [5, 3, 4]);
        assert!(text(&overlay, 2).contains("/Count 3"), "a move changes no count");

        let mut overlay = CowOverlay::new();
        build_move_pages_group(&flat3(), &[0], 2).unwrap().apply(&mut overlay).unwrap();
        assert_eq!(kids_of(&overlay, 2), [4, 5, 3]);
    }

    #[test]
    fn several_pages_move_together_and_keep_their_order() {
        let mut overlay = CowOverlay::new();
        build_move_pages_group(&flat3(), &[0, 1], 1).unwrap().apply(&mut overlay).unwrap();
        assert_eq!(kids_of(&overlay, 2), [5, 3, 4], "pages 1 and 2 land after page 3, still in order");
    }

    #[test]
    fn a_move_within_one_branch_of_a_nested_tree_touches_only_that_branch() {
        let group = build_move_pages_group(&nested(), &[1], 0).unwrap();
        assert_eq!(group.len(), 1, "only branch A is rewritten");
        let mut overlay = CowOverlay::new();
        group.apply(&mut overlay).unwrap();
        assert_eq!(kids_of(&overlay, 3), [6, 5]);
        assert!(overlay.get_object(2).is_none() && overlay.get_object(4).is_none());
    }

    #[test]
    fn a_move_that_would_change_a_pages_parent_is_refused() {
        let err = build_move_pages_group(&nested(), &[0], 3).unwrap_err();
        assert!(err.contains("branch"), "moving page 1 to the end crosses into branch B: {err}");
        let err = build_move_pages_group(&nested(), &[0, 2], 0).unwrap_err();
        assert!(err.contains("different branches"), "{err}");
    }

    #[test]
    fn a_move_that_changes_nothing_or_goes_nowhere_is_refused_by_name() {
        assert!(build_move_pages_group(&flat3(), &[1], 1).unwrap_err().contains("already there"));
        assert!(build_move_pages_group(&flat3(), &[0], 9).unwrap_err().contains("past the end"));
        assert!(build_move_pages_group(&flat3(), &[7], 0).unwrap_err().contains("page 8"));
        assert!(build_move_pages_group(&flat3(), &[], 0).is_err());
    }

    #[test]
    fn a_move_can_be_undone_exactly() {
        let nodes = flat3();
        let original = nodes[0].bytes.clone();
        let group = build_move_pages_group(&nodes, &[2], 0).unwrap();
        let mut overlay = CowOverlay::new();
        group.apply(&mut overlay).unwrap();
        group.undo(&mut overlay).unwrap();
        assert_eq!(overlay.get_object(2).unwrap(), original.as_slice());
    }

    #[test]
    fn duplicating_inserts_a_copy_right_after_each_original_and_counts_it() {
        let (group, next) = build_duplicate_pages_group(&flat3(), &[0, 1], 10).unwrap();
        assert_eq!(next, 12, "two objects were allocated");
        let mut overlay = CowOverlay::new();
        group.apply(&mut overlay).unwrap();
        assert_eq!(kids_of(&overlay, 2), [3, 10, 4, 11, 5]);
        assert!(text(&overlay, 2).contains("/Count 5"));
        let copy = text(&overlay, 10);
        assert!(copy.starts_with("10 0 obj\n") && copy.contains("/Type /Page") && copy.contains("/Parent 2 0 R"), "{copy}");
    }

    #[test]
    fn duplicating_in_a_nested_tree_counts_every_ancestor() {
        let (group, _) = build_duplicate_pages_group(&nested(), &[0], 20).unwrap();
        let mut overlay = CowOverlay::new();
        group.apply(&mut overlay).unwrap();
        assert_eq!(kids_of(&overlay, 3), [5, 20, 6]);
        assert!(text(&overlay, 3).contains("/Count 3"));
        assert!(text(&overlay, 2).contains("/Count 5"), "the root counts pages");
        assert!(overlay.get_object(4).is_none(), "branch B is untouched");
    }

    #[test]
    fn a_page_with_annotations_is_not_duplicated() {
        let mut nodes = flat3();
        nodes[2] = node(4, "<< /Type /Page /Parent 2 0 R /Annots [9 0 R] >>");
        let err = build_duplicate_pages_group(&nodes, &[1], 10).unwrap_err();
        assert!(err.contains("annotations"), "{err}");
    }

    #[test]
    fn a_blank_page_goes_where_asked_and_takes_its_neighbours_size() {
        for (at, expected) in [(0u32, vec![10, 3, 4, 5]), (1, vec![3, 10, 4, 5]), (3, vec![3, 4, 5, 10])] {
            let (group, next) = build_insert_blank_group(&flat3(), at, None, 10).unwrap();
            assert_eq!(next, 11);
            let mut overlay = CowOverlay::new();
            group.apply(&mut overlay).unwrap();
            assert_eq!(kids_of(&overlay, 2), expected, "inserted at {at}");
            assert!(text(&overlay, 2).contains("/Count 4"));
            let blank = text(&overlay, 10);
            assert!(blank.contains("/MediaBox [0 0 612 792]") && blank.contains("/Parent 2 0 R"), "{blank}");
        }
    }

    #[test]
    fn a_blank_page_in_a_nested_tree_joins_its_neighbours_branch() {
        // Before page 3 (the first page of branch B).
        let (group, _) = build_insert_blank_group(&nested(), 2, Some([100.0, 200.5]), 30).unwrap();
        let mut overlay = CowOverlay::new();
        group.apply(&mut overlay).unwrap();
        assert_eq!(kids_of(&overlay, 4), [30, 7, 8]);
        assert!(text(&overlay, 4).contains("/Count 3") && text(&overlay, 2).contains("/Count 5"));
        assert!(text(&overlay, 30).contains("/MediaBox [0 0 100 200.5]") && text(&overlay, 30).contains("/Parent 4 0 R"));
    }

    #[test]
    fn a_blank_page_takes_the_size_the_neighbour_inherits_and_bad_sizes_are_refused() {
        let nodes = vec![
            node(2, "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 300 400] >>"),
            node(3, "<< /Type /Page /Parent 2 0 R >>"),
        ];
        let (group, _) = build_insert_blank_group(&nodes, 1, None, 9).unwrap();
        let mut overlay = CowOverlay::new();
        group.apply(&mut overlay).unwrap();
        assert!(text(&overlay, 9).contains("/MediaBox [0 0 300 400]"), "{}", text(&overlay, 9));
        assert!(build_insert_blank_group(&nodes, 5, None, 9).unwrap_err().contains("past the end"));
        assert!(build_insert_blank_group(&nodes, 0, Some([0.0, 100.0]), 9).is_err());
        assert!(build_insert_blank_group(&nodes, 0, Some([f64::NAN, 100.0]), 9).is_err());
    }}
