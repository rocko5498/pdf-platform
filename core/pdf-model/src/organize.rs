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

/// Rotate one or more pages by 90-degree increments.
///
/// Modifies the /Rotate entry of each affected page object.
#[derive(Debug, Clone)]
pub struct RotatePagesCommand {
    /// (page_index, rotation_delta) pairs. Delta is +90, +180, or +270.
    pub rotations: Vec<(u32, u32)>,
    /// Page object numbers and their original rotation values.
    pub page_obj_rotations: Vec<(u32, u32)>,
    /// New rotation values after the operation.
    pub new_rotations: Vec<(u32, u32)>,
}

impl Command for RotatePagesCommand {
    fn name(&self) -> &str {
        "RotatePages"
    }

    fn apply(&self, overlay: &mut CowOverlay) -> Result<(), CommandError> {
        for &(obj_num, new_rot) in &self.new_rotations {
            let rot_bytes = format!("{} 0 obj\n<< /Rotate {} >>\nendobj\n", obj_num, new_rot);
            overlay.set_object(obj_num, rot_bytes.into_bytes());
        }
        Ok(())
    }

    fn undo(&self, overlay: &mut CowOverlay) -> Result<(), CommandError> {
        for &(obj_num, old_rot) in &self.page_obj_rotations {
            let rot_bytes = format!("{} 0 obj\n<< /Rotate {} >>\nendobj\n", obj_num, old_rot);
            overlay.set_object(obj_num, rot_bytes.into_bytes());
        }
        Ok(())
    }

    fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        use std::io::Write;
        for ((page, _delta), (_obj, new_rot)) in self.rotations.iter().zip(self.new_rotations.iter()) {
            let _ = writeln!(buf, "PAGE:{page}:ROT:{new_rot}");
        }
        buf
    }

    fn box_clone(&self) -> Box<dyn Command> {
        Box::new(self.clone())
    }
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

/// Helper to build a rotate-pages command group.
///
/// Given page object numbers and their current rotations, produces
/// a CommandGroup that applies the rotation delta.
pub fn build_rotate_pages_group(
    page_indices: &[u32],
    page_obj_rotations: &[(u32, u32)], // (page_index, current_rotation)
    degrees: u32,
) -> Result<CommandGroup, String> {
    if page_indices.is_empty() {
        return Err("no pages to rotate".into());
    }
    if !matches!(degrees, 90 | 180 | 270) {
        return Err(format!("invalid rotation: {degrees} (must be 90, 180, or 270)"));
    }

    let rotations: Vec<(u32, u32)> = page_indices.iter()
        .map(|&idx| (idx, degrees))
        .collect();

    let new_rotations: Vec<(u32, u32)> = page_indices.iter()
        .filter_map(|&idx| {
            page_obj_rotations.iter()
                .find(|(page, _)| *page == idx)
                .map(|(_, current)| (idx, (current + degrees) % 360))
        })
        .collect();

    let page_obj_rotations_owned: Vec<(u32, u32)> = page_indices.iter()
        .filter_map(|&idx| {
            page_obj_rotations.iter()
                .find(|(page, _)| *page == idx)
                .map(|(_, current)| (idx, *current))
        })
        .collect();

    let cmd = RotatePagesCommand {
        rotations,
        page_obj_rotations: page_obj_rotations_owned,
        new_rotations,
    };

    let mut group = CommandGroup::new(format!("Rotate {} page(s) by {degrees}°", page_indices.len()));
    group.push(Box::new(cmd));
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
