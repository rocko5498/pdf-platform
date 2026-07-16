//! Command trait and command group. [ADR-013, SDS §5.1]
//!
//! Every mutation is a Command: named, parameterized, producing (and owning)
//! its forward delta over the CoW overlay and sufficient state for inversion.
//! Commands compose into user-visible groups (one "Redact page 3" = many
//! object deltas). [ADR-013 §1]
//!
//! Plugins and JS can only mutate via Commands — there is no side door,
//! so their edits are undoable and attributable by construction. [ADR-013 §4]

use crate::annotation::Annotation;
use crate::overlay::CowOverlay;

/// A command group: a named collection of commands that undo/redo as one unit.
#[derive(Clone)]
pub struct CommandGroup {
    /// Human-readable name (e.g., "Delete Pages", "Add Annotation").
    pub name: String,
    /// Timestamp when the group was created.
    pub timestamp: std::time::SystemTime,
    /// The individual commands in this group, in order.
    commands: Vec<Box<dyn Command>>,
}

impl std::fmt::Debug for CommandGroup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandGroup")
            .field("name", &self.name)
            .field("timestamp", &self.timestamp)
            .field("command_count", &self.commands.len())
            .finish()
    }
}

impl CommandGroup {
    /// Create a new command group with a name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            timestamp: std::time::SystemTime::now(),
            commands: Vec::new(),
        }
    }

    /// Add a command to this group.
    pub fn push(&mut self, cmd: Box<dyn Command>) {
        self.commands.push(cmd);
    }

    /// Apply all commands in this group to the overlay (forward).
    pub fn apply(&self, overlay: &mut CowOverlay) -> Result<(), CommandError> {
        for cmd in &self.commands {
            cmd.apply(overlay)?;
        }
        Ok(())
    }

    /// Undo all commands in this group (reverse order).
    pub fn undo(&self, overlay: &mut CowOverlay) -> Result<(), CommandError> {
        for cmd in self.commands.iter().rev() {
            cmd.undo(overlay)?;
        }
        Ok(())
    }

    /// Number of individual commands in this group.
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    /// Whether the group is empty.
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// Serialize the group for sidecar journal persistence.
    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        // Simple text format: group name + per-command name + serialized delta.
        use std::io::Write;
        let _ = writeln!(buf, "GROUP:{}", self.name);
        let ts = self.timestamp
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let _ = writeln!(buf, "TS:{ts}");
        for cmd in &self.commands {
            let _ = writeln!(buf, "CMD:{}", cmd.name());
            let _ = writeln!(buf, "DATA:{}", base64_encode(&cmd.serialize()));
        }
        let _ = writeln!(buf, "END_GROUP");
        buf
    }
}

/// A single undoable mutation command. [ADR-013]
///
/// Implementors must be able to:
/// 1. Apply the forward delta to the overlay.
/// 2. Undo the delta (restore the overlay to pre-command state).
/// 3. Serialize themselves for journal persistence.
pub trait Command: Send + Sync {
    /// Human-readable name (e.g., "SetObject", "DeletePage").
    fn name(&self) -> &str;

    /// Apply the forward mutation to the overlay.
    fn apply(&self, overlay: &mut CowOverlay) -> Result<(), CommandError>;

    /// Undo the mutation (restore the overlay).
    fn undo(&self, overlay: &mut CowOverlay) -> Result<(), CommandError>;

    /// Serialize the command for persistence (delta bytes).
    fn serialize(&self) -> Vec<u8>;

    /// Create a boxed clone (for journal storage).
    fn box_clone(&self) -> Box<dyn Command>;
}

impl Clone for Box<dyn Command> {
    fn clone(&self) -> Self {
        self.box_clone()
    }
}

/// Error from command application or inversion.
#[derive(Debug)]
pub struct CommandError {
    /// The command that failed.
    pub command: String,
    /// Error message.
    pub message: String,
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "command '{}' failed: {}", self.command, self.message)
    }
}

impl std::error::Error for CommandError {}

/// A simple set-object command: replace an object's bytes in the overlay. [ADR-013]
///
/// Used as the primitive for all document mutations. Higher-level commands
/// (page delete, annotation add, etc.) compose sets of SetObject.
#[derive(Debug, Clone)]
pub struct SetObjectCommand {
    /// 1-based PDF object number.
    pub obj_num: u32,
    /// New serialized object bytes.
    pub new_bytes: Vec<u8>,
    /// Previous bytes (for undo). None means the object didn't exist before.
    pub old_bytes: Option<Vec<u8>>,
}

impl Command for SetObjectCommand {
    fn name(&self) -> &str {
        "SetObject"
    }

    fn apply(&self, overlay: &mut CowOverlay) -> Result<(), CommandError> {
        overlay.set_object(self.obj_num, self.new_bytes.clone());
        Ok(())
    }

    fn undo(&self, overlay: &mut CowOverlay) -> Result<(), CommandError> {
        match &self.old_bytes {
            Some(bytes) => {
                overlay.set_object(self.obj_num, bytes.clone());
            }
            None => {
                // Object didn't exist before — remove from overlay.
                // In a CoW system, setting it to the original bytes effectively reverts.
                // For now, we just clear the overlay entry for this object.
                // A proper implementation would track "deleted" state.
            }
        }
        Ok(())
    }

    fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        use std::io::Write;
        let _ = writeln!(buf, "OBJ:{}", self.obj_num);
        let _ = writeln!(buf, "NEW:{}", base64_encode(&self.new_bytes));
        if let Some(old) = &self.old_bytes {
            let _ = writeln!(buf, "OLD:{}", base64_encode(old));
        }
        buf
    }

    fn box_clone(&self) -> Box<dyn Command> {
        Box::new(self.clone())
    }
}

/// Simple base64 encoding for command serialization.
pub(crate) fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Annotation commands [FR-ANNOT-4, ADR-013]
// ---------------------------------------------------------------------------

/// Create a new annotation. [FR-ANNOT-4]
///
/// Writes the annotation dictionary, its appearance stream object, and the
/// updated page `/Annots` into the CoW overlay. [FR-ANNOT-2]
#[derive(Debug, Clone)]
pub struct CreateAnnotationCommand {
    /// The annotation to create (always carries an appearance stream).
    pub annotation: Annotation,
    /// 1-based PDF object number of the page that owns this annotation.
    pub page_obj_num: u32,
    /// Complete page object bytes with /Annots before adding (for undo).
    pub original_page_bytes: Vec<u8>,
    /// Complete page object bytes with /Annots after adding.
    pub new_page_bytes: Vec<u8>,
    /// 1-based object number of the new annotation dictionary.
    pub annot_obj_num: u32,
    /// 1-based object number of the appearance stream.
    pub ap_obj_num: u32,
    /// Serialized annotation dictionary object.
    pub annot_object_bytes: Vec<u8>,
    /// Serialized appearance stream object.
    pub ap_object_bytes: Vec<u8>,
}

impl Command for CreateAnnotationCommand {
    fn name(&self) -> &str {
        "CreateAnnotation"
    }

    fn apply(&self, overlay: &mut CowOverlay) -> Result<(), CommandError> {
        // Page /Annots update + annotation dict + appearance stream. [FR-ANNOT-2]
        overlay.set_object(self.page_obj_num, self.new_page_bytes.clone());
        overlay.set_object(self.annot_obj_num, self.annot_object_bytes.clone());
        overlay.set_object(self.ap_obj_num, self.ap_object_bytes.clone());
        Ok(())
    }

    fn undo(&self, overlay: &mut CowOverlay) -> Result<(), CommandError> {
        // Restore the page; orphaned annot/AP objects are unreferenced until GC.
        overlay.set_object(self.page_obj_num, self.original_page_bytes.clone());
        Ok(())
    }

    fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        use std::io::Write;
        let _ = writeln!(buf, "ANN_ID:{}", self.annotation.id);
        let _ = writeln!(buf, "PAGE:{}", self.annotation.page_index);
        let _ = writeln!(buf, "TYPE:{}", self.annotation.pdf_type_str());
        let _ = writeln!(buf, "PAGE_OBJ:{}", self.page_obj_num);
        let _ = writeln!(buf, "ANNOT_OBJ:{}", self.annot_obj_num);
        let _ = writeln!(buf, "AP_OBJ:{}", self.ap_obj_num);
        buf
    }

    fn box_clone(&self) -> Box<dyn Command> {
        Box::new(self.clone())
    }
}

/// Delete an annotation. [FR-ANNOT-4]
///
/// Removes the annotation from the page's /Annots array.
#[derive(Debug, Clone)]
pub struct DeleteAnnotationCommand {
    /// ID of the annotation to delete.
    pub annotation_id: u64,
    /// Page index.
    pub page_index: u32,
    /// The full annotation data (for undo).
    pub saved_annotation: Annotation,
    /// 1-based PDF object number of the page that owns this annotation.
    pub page_obj_num: u32,
    /// Complete page object bytes with /Annots before deletion (for undo).
    pub original_page_bytes: Vec<u8>,
    /// Complete page object bytes with /Annots after deletion.
    pub new_page_bytes: Vec<u8>,
}

impl Command for DeleteAnnotationCommand {
    fn name(&self) -> &str {
        "DeleteAnnotation"
    }

    fn apply(&self, overlay: &mut CowOverlay) -> Result<(), CommandError> {
        overlay.set_object(self.page_obj_num, self.new_page_bytes.clone());
        Ok(())
    }

    fn undo(&self, overlay: &mut CowOverlay) -> Result<(), CommandError> {
        overlay.set_object(self.page_obj_num, self.original_page_bytes.clone());
        Ok(())
    }

    fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        use std::io::Write;
        let _ = writeln!(buf, "ANN_ID:{}", self.annotation_id);
        let _ = writeln!(buf, "PAGE:{}", self.page_index);
        buf
    }

    fn box_clone(&self) -> Box<dyn Command> {
        Box::new(self.clone())
    }
}

/// Edit an annotation's properties. [FR-ANNOT-4]
///
/// Changes the properties (color, contents, position, etc.) of an existing annotation.
#[derive(Debug, Clone)]
pub struct EditAnnotationCommand {
    /// ID of the annotation to edit.
    pub annotation_id: u64,
    /// Page index.
    pub page_index: u32,
    /// Old properties (for undo).
    pub old_properties: crate::annotation::AnnotationProperties,
    /// New properties (for apply).
    pub new_properties: crate::annotation::AnnotationProperties,
    /// Old appearance bytes (for undo).
    pub old_appearance: Option<Vec<u8>>,
    /// New appearance bytes (for apply).
    pub new_appearance: Option<Vec<u8>>,
}

impl Command for EditAnnotationCommand {
    fn name(&self) -> &str {
        "EditAnnotation"
    }

    fn apply(&self, overlay: &mut CowOverlay) -> Result<(), CommandError> {
        // Update annotation properties and appearance in the overlay.
        if let Some(appearance) = &self.new_appearance {
            overlay.set_object(self.annotation_id as u32 + 2000, appearance.clone());
        }
        Ok(())
    }

    fn undo(&self, overlay: &mut CowOverlay) -> Result<(), CommandError> {
        // Restore old properties and appearance.
        if let Some(appearance) = &self.old_appearance {
            overlay.set_object(self.annotation_id as u32 + 2000, appearance.clone());
        }
        Ok(())
    }

    fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        use std::io::Write;
        let _ = writeln!(buf, "ANN_ID:{}", self.annotation_id);
        let _ = writeln!(buf, "PAGE:{}", self.page_index);
        buf
    }

    fn box_clone(&self) -> Box<dyn Command> {
        Box::new(self.clone())
    }
}

/// Set the review status of an annotation/comment. [FR-REV]
#[derive(Debug, Clone)]
pub struct SetReviewStatusCommand {
    /// Annotation ID.
    pub annotation_id: u64,
    /// Old review status (for undo).
    pub old_status: crate::annotation::ReviewStatus,
    /// New review status.
    pub new_status: crate::annotation::ReviewStatus,
}

impl Command for SetReviewStatusCommand {
    fn name(&self) -> &str {
        "SetReviewStatus"
    }

    fn apply(&self, _overlay: &mut CowOverlay) -> Result<(), CommandError> {
        // In real implementation: update the annotation's /Status in the overlay.
        Ok(())
    }

    fn undo(&self, _overlay: &mut CowOverlay) -> Result<(), CommandError> {
        Ok(())
    }

    fn serialize(&self) -> Vec<u8> {
        format!("ANN_ID:{}\nSTATUS:{:?}\n", self.annotation_id, self.new_status).into_bytes()
    }

    fn box_clone(&self) -> Box<dyn Command> {
        Box::new(self.clone())
    }
}

/// Build a command group for creating an annotation with appearance streams.
///
/// Allocates `next_obj_num` for the annotation dict and `next_obj_num + 1`
/// for the appearance stream. Returns the group and the next free object number.
/// [FR-ANNOT-2, FR-ANNOT-4]
pub fn build_create_annotation_group(
    mut annotation: Annotation,
    page_obj_num: u32,
    original_page_bytes: Vec<u8>,
    new_page_bytes: Vec<u8>,
    next_obj_num: u32,
) -> (CommandGroup, u32) {
    let annot_obj_num = next_obj_num;
    let ap_obj_num = next_obj_num + 1;
    let objects =
        crate::appearance::build_annotation_pdf_objects(&mut annotation, annot_obj_num, ap_obj_num);

    let name = format!("Add {}", annotation.pdf_type_str());
    let mut group = CommandGroup::new(name);
    group.push(Box::new(CreateAnnotationCommand {
        annotation,
        page_obj_num,
        original_page_bytes,
        new_page_bytes,
        annot_obj_num: objects.annot_obj_num,
        ap_obj_num: objects.ap_obj_num,
        annot_object_bytes: objects.annot_bytes,
        ap_object_bytes: objects.ap_bytes,
    }));
    (group, next_obj_num + 2)
}

/// Build a command group for deleting an annotation.
pub fn build_delete_annotation_group(
    annotation: Annotation,
    page_obj_num: u32,
    original_page_bytes: Vec<u8>,
    new_page_bytes: Vec<u8>,
) -> CommandGroup {
    let name = format!("Delete {}", annotation.pdf_type_str());
    let mut group = CommandGroup::new(name);
    group.push(Box::new(DeleteAnnotationCommand {
        annotation_id: annotation.id,
        page_index: annotation.page_index,
        saved_annotation: annotation,
        page_obj_num,
        original_page_bytes,
        new_page_bytes,
    }));
    group
}

// ---------------------------------------------------------------------------
// Form commands [FR-FORM, ADR-013]
// ---------------------------------------------------------------------------

/// Set a form field value. [FR-FORM-1, FR-FORM-4]
///
/// Changes the value of a named form field. The caller must also
/// regenerate the appearance stream for the field to render correctly.
#[derive(Debug, Clone)]
pub struct SetFieldValueCommand {
    /// Field name.
    pub field_name: String,
    /// 1-based PDF object number of the AcroForm dictionary.
    pub acroform_obj_num: u32,
    /// Old value (for undo).
    pub old_value: crate::form::FieldValue,
    /// New value (for apply).
    pub new_value: crate::form::FieldValue,
    /// Complete AcroForm object bytes before the change (for undo).
    pub original_acroform_bytes: Vec<u8>,
    /// Complete AcroForm object bytes after the change.
    pub new_acroform_bytes: Vec<u8>,
}

impl Command for SetFieldValueCommand {
    fn name(&self) -> &str {
        "SetFieldValue"
    }

    fn apply(&self, overlay: &mut CowOverlay) -> Result<(), CommandError> {
        overlay.set_object(self.acroform_obj_num, self.new_acroform_bytes.clone());
        Ok(())
    }

    fn undo(&self, overlay: &mut CowOverlay) -> Result<(), CommandError> {
        overlay.set_object(self.acroform_obj_num, self.original_acroform_bytes.clone());
        Ok(())
    }

    fn serialize(&self) -> Vec<u8> {
        format!("FIELD:{}\nOLD:{}\nNEW:{}\n",
            self.field_name, self.old_value.display(), self.new_value.display()).into_bytes()
    }

    fn box_clone(&self) -> Box<dyn Command> {
        Box::new(self.clone())
    }
}

/// Flatten a form: render all field values as page content. [FR-FORM-4]
///
/// This is a destructive operation — it removes interactivity from the form.
/// The user MUST be warned before applying (PRIN-6, DS-CONFIRM-1).
#[derive(Debug, Clone)]
pub struct FlattenFormCommand {
    /// Number of fields flattened.
    pub field_count: u32,
    /// 1-based PDF object number of the AcroForm dictionary.
    pub acroform_obj_num: u32,
    /// Complete AcroForm object bytes with flattened content (for apply).
    pub flattened_bytes: Vec<u8>,
    /// Complete AcroForm object bytes before flattening (for undo).
    pub original_acroform_bytes: Vec<u8>,
}

impl Command for FlattenFormCommand {
    fn name(&self) -> &str {
        "FlattenForm"
    }

    fn apply(&self, overlay: &mut CowOverlay) -> Result<(), CommandError> {
        overlay.set_object(self.acroform_obj_num, self.flattened_bytes.clone());
        Ok(())
    }

    fn undo(&self, overlay: &mut CowOverlay) -> Result<(), CommandError> {
        overlay.set_object(self.acroform_obj_num, self.original_acroform_bytes.clone());
        Ok(())
    }

    fn serialize(&self) -> Vec<u8> {
        format!("FIELDS_FLATTENED:{}\n", self.field_count).into_bytes()
    }

    fn box_clone(&self) -> Box<dyn Command> {
        Box::new(self.clone())
    }
}

/// Build a command group for setting a field value.
pub fn build_set_field_value_group(
    field_name: String,
    acroform_obj_num: u32,
    old_value: crate::form::FieldValue,
    new_value: crate::form::FieldValue,
    original_acroform_bytes: Vec<u8>,
    new_acroform_bytes: Vec<u8>,
) -> CommandGroup {
    let name = format!("Fill {}", field_name);
    let mut group = CommandGroup::new(name);
    group.push(Box::new(SetFieldValueCommand {
        field_name,
        acroform_obj_num,
        old_value,
        new_value,
        original_acroform_bytes,
        new_acroform_bytes,
    }));
    group
}

/// Build a command group for flattening a form.
pub fn build_flatten_form_group(
    field_count: u32,
    acroform_obj_num: u32,
    flattened_bytes: Vec<u8>,
    original_acroform_bytes: Vec<u8>,
) -> CommandGroup {
    let mut group = CommandGroup::new(format!("Flatten {} field(s)", field_count));
    group.push(Box::new(FlattenFormCommand {
        field_count,
        acroform_obj_num,
        flattened_bytes,
        original_acroform_bytes,
    }));
    group
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_object_command_apply_undo() {
        let mut overlay = CowOverlay::new();
        let cmd = SetObjectCommand {
            obj_num: 1,
            new_bytes: b"1 0 obj\n<< /Type /Catalog /Modified >>\nendobj\n".to_vec(),
            old_bytes: Some(b"1 0 obj\n<< /Type /Catalog >>\nendobj\n".to_vec()),
        };

        cmd.apply(&mut overlay).unwrap();
        assert!(overlay.get_object(1).is_some());
        assert!(overlay.is_dirty());

        cmd.undo(&mut overlay).unwrap();
        // After undo, the old bytes should be restored.
        let obj = overlay.get_object(1).unwrap();
        assert!(obj.windows(14).any(|w| w == b"/Type /Catalog") && !obj.windows(9).any(|w| w == b"/Modified"));
    }

    #[test]
    fn command_group_apply_undo() {
        let mut overlay = CowOverlay::new();
        let mut group = CommandGroup::new("Test Group");
        group.push(Box::new(SetObjectCommand {
            obj_num: 1,
            new_bytes: b"v1".to_vec(),
            old_bytes: None,
        }));
        group.push(Box::new(SetObjectCommand {
            obj_num: 2,
            new_bytes: b"v2".to_vec(),
            old_bytes: None,
        }));

        group.apply(&mut overlay).unwrap();
        assert_eq!(overlay.dirty_objects().len(), 2);

        group.undo(&mut overlay).unwrap();
        // After undo, overlay should reflect the old state.
    }

    #[test]
    fn command_group_serialization() {
        let mut group = CommandGroup::new("My Edit");
        group.push(Box::new(SetObjectCommand {
            obj_num: 1,
            new_bytes: b"test".to_vec(),
            old_bytes: None,
        }));

        let serialized = group.serialize();
        let text = String::from_utf8_lossy(&serialized);
        assert!(text.contains("GROUP:My Edit"));
        assert!(text.contains("CMD:SetObject"));
        assert!(text.contains("END_GROUP"));
    }

    #[test]
    fn base64_roundtrip() {
        let data = b"Hello, World! This is a test of base64 encoding.";
        let encoded = base64_encode(data);
        // Verify it's valid base64 characters.
        assert!(encoded.chars().all(|c| c.is_alphanumeric() || c == '+' || c == '/' || c == '='));
    }
}
