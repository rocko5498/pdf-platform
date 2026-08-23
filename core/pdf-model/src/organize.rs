//! Page organization commands: reorder, rotate, delete, insert. [FR-ORG, M3]
//!
//! These are the simplest real edits to exercise the CoW overlay + command
//! journal pipeline end-to-end. Each operation is expressed as a set of
//! SetObjectCommands that modify the Pages tree. [ADR-012, SDS §3.4]

use crate::command::{Command, CommandError, CommandGroup, base64_encode};
use crate::overlay::CowOverlay;

/// Delete one or more pages by their 0-based indices.
///
/// This modifies the Pages parent object's /Kids array to remove the
/// specified page references. The deleted page objects themselves are
/// NOT removed from the file (they become orphaned) — this preserves
/// the original bytes for undo. [ADR-006]
#[derive(Debug, Clone)]
pub struct DeletePagesCommand {
    /// 0-based page indices to delete, sorted ascending.
    pub page_indices: Vec<u32>,
    /// The Pages parent object number (typically object 2).
    pub pages_obj_num: u32,
    /// Original /Kids array bytes (for undo).
    pub original_kids: Vec<u8>,
    /// New /Kids array bytes after deletion.
    pub new_kids: Vec<u8>,
}

impl Command for DeletePagesCommand {
    fn name(&self) -> &str {
        "DeletePages"
    }

    fn apply(&self, overlay: &mut CowOverlay) -> Result<(), CommandError> {
        overlay.set_object(self.pages_obj_num, self.new_kids.clone());
        Ok(())
    }

    fn undo(&self, overlay: &mut CowOverlay) -> Result<(), CommandError> {
        overlay.set_object(self.pages_obj_num, self.original_kids.clone());
        Ok(())
    }

    fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        use std::io::Write;
        let _ = writeln!(buf, "PAGES_OBJ:{}", self.pages_obj_num);
        let _ = writeln!(buf, "DELETED:{}", self.page_indices.iter()
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(","));
        let _ = writeln!(buf, "NEW_KIDS:{}", base64_encode(&self.new_kids));
        let _ = writeln!(buf, "OLD_KIDS:{}", base64_encode(&self.original_kids));
        buf
    }

    fn box_clone(&self) -> Box<dyn Command> {
        Box::new(self.clone())
    }
}

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

/// Helper to build a delete-pages command group.
///
/// Given the current Kids array bytes and the pages to delete, produces
/// a CommandGroup that can be applied and undone.
pub fn build_delete_pages_group(
    page_indices: &[u32],
    pages_obj_num: u32,
    current_kids: &[u8],
    _total_pages: u32,
) -> Result<CommandGroup, String> {
    if page_indices.is_empty() {
        return Err("no pages to delete".into());
    }

    let kids_text = String::from_utf8_lossy(current_kids);

    // Parse existing kid references from the Kids array.
    let mut kid_refs: Vec<String> = Vec::new();
    let in_array = kids_text.contains("/Kids [");
    if in_array {
        // Extract references between [ and ].
        // PDF references are "N G R" (object number, generation, "R").
        if let Some(start) = kids_text.find("/Kids [") {
            let array_start = start + "/Kids [".len();
            if let Some(end) = kids_text[array_start..].find(']') {
                let array = &kids_text[array_start..array_start + end];
                let tokens: Vec<&str> = array.split_whitespace().collect();
                // Group into triplets: "N", "G", "R"
                for chunk in tokens.chunks(3) {
                    if chunk.len() == 3 && chunk[2] == "R" {
                        kid_refs.push(format!("{} {} R", chunk[0], chunk[1]));
                    }
                }
            }
        }
    }

    if kid_refs.is_empty() {
        return Err("could not parse Kids array".into());
    }

    // Build the deleted set.
    let delete_set: std::collections::HashSet<u32> = page_indices.iter().copied().collect();

    // Filter out deleted pages.
    let new_kids_refs: Vec<&str> = kid_refs.iter()
        .enumerate()
        .filter(|(i, _)| !delete_set.contains(&(*i as u32)))
        .map(|(_, r)| r.as_str())
        .collect();

    // Reconstruct the Kids array.
    let new_kids_array = format!(
        "{} 0 obj\n<< /Type /Pages /Kids [{}] /Count {} >>\nendobj\n",
        pages_obj_num,
        new_kids_refs.join(" "),
        new_kids_refs.len()
    );

    let cmd = DeletePagesCommand {
        page_indices: page_indices.to_vec(),
        pages_obj_num,
        original_kids: current_kids.to_vec(),
        new_kids: new_kids_array.into_bytes(),
    };

    let mut group = CommandGroup::new(format!("Delete {} page(s)", page_indices.len()));
    group.push(Box::new(cmd));
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

    #[test]
    fn delete_pages_command() {
        let mut overlay = CowOverlay::new();
        let cmd = DeletePagesCommand {
            page_indices: vec![1],
            pages_obj_num: 2,
            original_kids: b"old kids".to_vec(),
            new_kids: b"new kids".to_vec(),
        };

        cmd.apply(&mut overlay).unwrap();
        assert_eq!(overlay.get_object(2), Some(b"new kids".as_slice()));

        cmd.undo(&mut overlay).unwrap();
        assert_eq!(overlay.get_object(2), Some(b"old kids".as_slice()));
    }

    #[test]
    fn build_delete_group() {
        let kids = b"2 0 obj\n<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 >>\nendobj\n";
        let group = build_delete_pages_group(&[1], 2, kids, 3).unwrap();
        assert_eq!(group.name, "Delete 1 page(s)");
        assert_eq!(group.len(), 1);

        let mut overlay = CowOverlay::new();
        group.apply(&mut overlay).unwrap();
        let new_kids = String::from_utf8_lossy(overlay.get_object(2).unwrap()).to_string();
        assert!(new_kids.contains("/Count 2"));
        assert!(new_kids.contains("3 0 R"));
        assert!(new_kids.contains("5 0 R"));
        assert!(!new_kids.contains("4 0 R"));
    }
}
