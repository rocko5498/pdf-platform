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

/// Build a rotate-pages command group, patching each page in place.
///
/// `pages` is `(object number, current page bytes)` for every page to rotate.
/// The bytes are required for two reasons: rotation must preserve the rest of
/// the page dictionary, and undo must restore exactly what was there.
///
/// The previous signature took page *indices* alongside a list the caller
/// populated with page *object numbers*, and matched one against the other — so
/// the lookup never succeeded and rotation silently did nothing. Taking the
/// pages themselves removes the mismatch rather than documenting it.
/// [FR-ROTATE, ADR-012, PRIN-1]
pub fn build_rotate_pages_group(
    pages: &[(u32, Vec<u8>)],
    degrees: u32,
) -> Result<CommandGroup, String> {
    if pages.is_empty() {
        return Err("no pages to rotate".into());
    }
    if !matches!(degrees, 90 | 180 | 270) {
        return Err(format!("invalid rotation: {degrees} (must be 90, 180, or 270)"));
    }

    let mut group = CommandGroup::new(format!("Rotate {} page(s) by {degrees}°", pages.len()));
    for (obj_num, page_bytes) in pages {
        let rotation = (page_rotation(page_bytes) + degrees) % 360;
        let new_bytes = set_page_rotation(page_bytes, rotation)?;
        group.push(Box::new(crate::command::SetObjectCommand {
            obj_num: *obj_num,
            new_bytes,
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
}