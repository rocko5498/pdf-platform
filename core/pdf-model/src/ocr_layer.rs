//! Invisible OCR text-layer page patching. [FR-OCR-1, ADR-013]
//!
//! Wraps an already-generated content stream (`ocr_bridge::generate_text_layer_stream`)
//! as a new PDF stream object, links it onto a page's `/Contents`, and ensures the
//! `/F1` font resource it references exists — the same surgical byte-level dict
//! edits as [`crate::page_patch`], not a full re-parse.

use crate::command::{Command, CommandError};
use crate::overlay::CowOverlay;

/// Standard base-14 Helvetica font object referenced as `/F1` by
/// `ocr_bridge::generate_text_layer_stream`.
///
/// Invisible (`Tr 3`) text never renders, so glyph shapes don't matter;
/// `/WinAnsiEncoding` gives correct copy/extract for Latin-script text. Full
/// CJK/RTL extraction needs an embedded glyphless font — a tracked gap, not
/// silently claimed here.
pub fn build_standard_font_object(font_obj_num: u32) -> Vec<u8> {
    format!(
        "{font_obj_num} 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>\nendobj\n"
    )
    .into_bytes()
}

/// Wrap raw content-stream bytes as a complete PDF stream object.
pub fn build_content_stream_object(content_obj_num: u32, stream_bytes: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(stream_bytes.len() + 64);
    buf.extend_from_slice(format!("{content_obj_num} 0 obj\n<< /Length {} >>\nstream\n", stream_bytes.len()).as_bytes());
    buf.extend_from_slice(stream_bytes);
    buf.extend_from_slice(b"\nendstream\nendobj\n");
    buf
}

/// Failure patching a page for a text layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OcrLayerPatchError {
    /// `/Contents` array is unclosed (malformed page dict).
    UnclosedContentsArray,
    /// Page object is missing its closing `>>`.
    MissingPageDictClose,
    /// `/Resources` dict is unclosed.
    UnclosedResources,
    /// Page bytes are not valid UTF-8 (should never happen for our own writer output).
    InvalidUtf8,
}

impl std::fmt::Display for OcrLayerPatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

/// Append a content-stream object ref to a page's `/Contents` and ensure
/// `/Resources/Font/F1` points at `font_obj_num`. [FR-OCR-1]
pub fn inject_text_layer(
    page_obj_bytes: &[u8],
    content_obj_num: u32,
    font_obj_num: u32,
) -> Result<Vec<u8>, OcrLayerPatchError> {
    let text =
        std::str::from_utf8(page_obj_bytes).map_err(|_| OcrLayerPatchError::InvalidUtf8)?;
    let with_contents = inject_contents_ref(text, content_obj_num)?;
    let with_font = inject_font_resource(&with_contents, font_obj_num)?;
    Ok(with_font.into_bytes())
}

fn inject_contents_ref(text: &str, content_obj_num: u32) -> Result<String, OcrLayerPatchError> {
    let entry = format!("{content_obj_num} 0 R");
    let Some(key_idx) = text.find("/Contents") else {
        let insert_at = text
            .rfind(">>")
            .ok_or(OcrLayerPatchError::MissingPageDictClose)?;
        return Ok(format!(
            "{}/Contents [{}] {}",
            &text[..insert_at],
            entry,
            &text[insert_at..]
        ));
    };
    let after_key = key_idx + "/Contents".len();
    let value_start = after_key + text[after_key..].len() - text[after_key..].trim_start().len();
    if text[value_start..].starts_with('[') {
        let (open, close) = bracket_range(text, value_start, '[', ']')
            .ok_or(OcrLayerPatchError::UnclosedContentsArray)?;
        let existing = text[open + 1..close].trim();
        let new_arr = if existing.is_empty() {
            format!("[{entry}]")
        } else {
            format!("[{existing} {entry}]")
        };
        Ok(format!(
            "{}{}{}",
            &text[..open],
            new_arr,
            &text[close + 1..]
        ))
    } else {
        // Single indirect ref, e.g. "5 0 R" — extent ends at the next '/' or '>>'.
        let rest = &text[value_start..];
        let end_rel = rest
            .find('/')
            .into_iter()
            .chain(rest.find(">>"))
            .min()
            .unwrap_or(rest.len());
        let existing_ref = rest[..end_rel].trim();
        let value_end = value_start + end_rel;
        Ok(format!(
            "{}[{existing_ref} {entry}]{}",
            &text[..value_start],
            &text[value_end..]
        ))
    }
}

fn inject_font_resource(text: &str, font_obj_num: u32) -> Result<String, OcrLayerPatchError> {
    let font_entry = format!("/F1 {font_obj_num} 0 R");
    let Some(res_key) = text.find("/Resources") else {
        let insert_at = text
            .rfind(">>")
            .ok_or(OcrLayerPatchError::MissingPageDictClose)?;
        return Ok(format!(
            "{}/Resources << /Font << {font_entry} >> >> {}",
            &text[..insert_at],
            &text[insert_at..]
        ));
    };
    let res_dict_start = text[res_key..]
        .find("<<")
        .map(|i| res_key + i)
        .ok_or(OcrLayerPatchError::UnclosedResources)?;
    let (res_open, res_close) = dict_range(text, res_dict_start)
        .ok_or(OcrLayerPatchError::UnclosedResources)?;
    let resources_body = &text[res_open + 2..res_close];

    let new_body = if let Some(font_key) = resources_body.find("/Font") {
        let font_dict_start = resources_body[font_key..]
            .find("<<")
            .map(|i| font_key + i)
            .ok_or(OcrLayerPatchError::UnclosedResources)?;
        let (font_open, font_close) = dict_range(resources_body, font_dict_start)
            .ok_or(OcrLayerPatchError::UnclosedResources)?;
        if resources_body[font_open..=font_close].contains("/F1") {
            // Already registered (e.g. re-running OCR) — leave as-is.
            resources_body.to_string()
        } else {
            format!(
                "{}<< {} {} >>{}",
                &resources_body[..font_open],
                &resources_body[font_open + 2..font_close],
                font_entry,
                &resources_body[font_close + 2..]
            )
        }
    } else {
        format!("{resources_body} /Font << {font_entry} >>")
    };

    Ok(format!(
        "{}<<{}>>{}",
        &text[..res_open],
        new_body,
        &text[res_close + 2..]
    ))
}

/// Find the `(open, close)` byte indices of a bracket pair starting at or
/// after `start`, where `close` is the index of the matching closer.
fn bracket_range(text: &str, start: usize, open_ch: char, close_ch: char) -> Option<(usize, usize)> {
    let open = text[start..].find(open_ch)? + start;
    let bytes = text.as_bytes();
    let mut depth = 0i32;
    let mut i = open;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if c == open_ch {
            depth += 1;
        } else if c == close_ch {
            depth -= 1;
            if depth == 0 {
                return Some((open, i));
            }
        }
        i += 1;
    }
    None
}

/// Find the `(open, close)` byte indices of a `<< ... >>` dict starting at
/// `dict_start` (the index of its opening `<<`), tracking nesting depth so
/// an inner dict's `>>` doesn't prematurely close the outer one.
fn dict_range(text: &str, dict_start: usize) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut depth = 0i32;
    let mut i = dict_start;
    while i + 1 < bytes.len() {
        if &bytes[i..i + 2] == b"<<" {
            depth += 1;
            i += 2;
            continue;
        }
        if &bytes[i..i + 2] == b">>" {
            depth -= 1;
            if depth == 0 {
                return Some((dict_start, i));
            }
            i += 2;
            continue;
        }
        i += 1;
    }
    None
}

/// Apply a generated invisible text layer to one page. [FR-OCR-1, ADR-013]
///
/// Adds the content-stream object and (when not already present) the
/// standard font object; patches the page's `/Contents` and
/// `/Resources/Font`. Undo restores the original page bytes — the orphaned
/// content/font objects stay unreferenced until GC, matching
/// `DeleteAnnotationCommand`'s convention.
#[derive(Debug, Clone)]
pub struct ApplyOcrTextLayerCommand {
    /// Zero-based page index (diagnostics only).
    pub page_index: u32,
    /// 1-based object number of the page dictionary.
    pub page_obj_num: u32,
    /// Page object bytes before the text layer was linked (for undo).
    pub original_page_bytes: Vec<u8>,
    /// Page object bytes after linking (for apply).
    pub new_page_bytes: Vec<u8>,
    /// 1-based object number of the new content-stream object.
    pub content_obj_num: u32,
    /// Serialized content-stream object bytes.
    pub content_object_bytes: Vec<u8>,
    /// 1-based object number of the font object, if newly created this call
    /// (`None` when an existing `/F1` was reused).
    pub font_obj_num: Option<u32>,
    /// Serialized font object bytes, present iff `font_obj_num.is_some()`.
    pub font_object_bytes: Option<Vec<u8>>,
}

impl Command for ApplyOcrTextLayerCommand {
    fn name(&self) -> &str {
        "ApplyOcrTextLayer"
    }

    fn apply(&self, overlay: &mut CowOverlay) -> Result<(), CommandError> {
        overlay.set_object(self.page_obj_num, self.new_page_bytes.clone());
        overlay.set_object(self.content_obj_num, self.content_object_bytes.clone());
        if let (Some(font_obj_num), Some(bytes)) = (self.font_obj_num, &self.font_object_bytes) {
            overlay.set_object(font_obj_num, bytes.clone());
        }
        Ok(())
    }

    fn undo(&self, overlay: &mut CowOverlay) -> Result<(), CommandError> {
        overlay.set_object(self.page_obj_num, self.original_page_bytes.clone());
        Ok(())
    }

    fn serialize(&self) -> Vec<u8> {
        format!(
            "PAGE:{}\nPAGE_OBJ:{}\nCONTENT_OBJ:{}\n",
            self.page_index, self.page_obj_num, self.content_obj_num
        )
        .into_bytes()
    }

    fn box_clone(&self) -> Box<dyn Command> {
        Box::new(self.clone())
    }
}

/// Build a command group applying an OCR text layer to one page.
///
/// Always reserves two object numbers (content stream, font). If the page
/// already declares `/F1` (e.g. a prior OCR pass), [`inject_text_layer`]'s
/// dedup path leaves `/Resources` untouched and the reserved font object
/// number is simply never written — harmless, no caller-asserted flag to
/// get wrong.
pub fn build_apply_ocr_text_layer_group(
    page_index: u32,
    page_obj_num: u32,
    original_page_bytes: Vec<u8>,
    text_layer_stream: &[u8],
    next_obj_num: u32,
) -> Result<(crate::command::CommandGroup, u32), OcrLayerPatchError> {
    let content_obj_num = next_obj_num;
    let font_obj_num = next_obj_num + 1;

    let new_page_bytes = inject_text_layer(&original_page_bytes, content_obj_num, font_obj_num)?;
    let font_was_added = new_page_bytes
        .windows(format!("/F1 {font_obj_num} 0 R").len())
        .any(|w| w == format!("/F1 {font_obj_num} 0 R").as_bytes());

    let content_object_bytes = build_content_stream_object(content_obj_num, text_layer_stream);
    let (font_obj_num_opt, font_object_bytes) = if font_was_added {
        (
            Some(font_obj_num),
            Some(build_standard_font_object(font_obj_num)),
        )
    } else {
        (None, None)
    };

    let mut group = crate::command::CommandGroup::new(format!("OCR page {}", page_index + 1));
    group.push(Box::new(ApplyOcrTextLayerCommand {
        page_index,
        page_obj_num,
        original_page_bytes,
        new_page_bytes,
        content_obj_num,
        content_object_bytes,
        font_obj_num: font_obj_num_opt,
        font_object_bytes,
    }));
    Ok((group, next_obj_num + 2))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE_NO_RESOURCES: &[u8] =
        b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R >>\nendobj\n";
    const PAGE_WITH_RESOURCES: &[u8] = b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F2 9 0 R >> /ProcSet [/PDF /Text] >> >>\nendobj\n";
    const PAGE_CONTENTS_ARRAY: &[u8] = b"3 0 obj\n<< /Type /Page /Contents [4 0 R 5 0 R] /Resources << >> >>\nendobj\n";

    #[test]
    fn inject_text_layer_creates_contents_and_font_from_scratch() {
        let out = inject_text_layer(PAGE_NO_RESOURCES, 50, 51).unwrap();
        let s = String::from_utf8_lossy(&out);
        assert!(s.contains("/Contents [4 0 R 50 0 R]"), "got: {s}");
        assert!(s.contains("/Resources"), "got: {s}");
        assert!(s.contains("/F1 51 0 R"), "got: {s}");
        assert!(s.contains("/Type /Page"));
    }

    #[test]
    fn inject_text_layer_extends_contents_array() {
        let out = inject_text_layer(PAGE_CONTENTS_ARRAY, 50, 51).unwrap();
        let s = String::from_utf8_lossy(&out);
        assert!(s.contains("/Contents [4 0 R 5 0 R 50 0 R]"), "got: {s}");
        assert!(s.contains("/F1 51 0 R"), "got: {s}");
    }

    #[test]
    fn inject_text_layer_preserves_existing_font_and_procset() {
        let out = inject_text_layer(PAGE_WITH_RESOURCES, 50, 51).unwrap();
        let s = String::from_utf8_lossy(&out);
        assert!(s.contains("/F2 9 0 R"), "existing font must survive: {s}");
        assert!(s.contains("/F1 51 0 R"), "new font must be added: {s}");
        assert!(s.contains("/ProcSet [/PDF /Text]"), "unrelated resources survive: {s}");
        assert!(s.contains("/Contents [4 0 R 50 0 R]"), "got: {s}");
    }

    #[test]
    fn inject_text_layer_skips_duplicate_f1() {
        let already = b"3 0 obj\n<< /Type /Page /Contents 4 0 R /Resources << /Font << /F1 51 0 R >> >> >>\nendobj\n";
        let out = inject_text_layer(already, 50, 51).unwrap();
        let s = String::from_utf8_lossy(&out);
        // Exactly one /F1 entry — not duplicated.
        assert_eq!(s.matches("/F1").count(), 1, "got: {s}");
    }

    #[test]
    fn build_content_stream_object_has_correct_length() {
        let obj = build_content_stream_object(9, b"BT ET");
        let s = String::from_utf8_lossy(&obj);
        assert!(s.contains("/Length 5"));
        assert!(s.contains("9 0 obj"));
        assert!(s.contains("stream\nBT ET\nendstream\nendobj"));
    }

    #[test]
    fn apply_ocr_text_layer_command_apply_and_undo() {
        let mut overlay = CowOverlay::new();
        let (group, next) = build_apply_ocr_text_layer_group(
            0,
            3,
            PAGE_NO_RESOURCES.to_vec(),
            b"BT 3 Tr ET",
            50,
        )
        .unwrap();
        assert_eq!(next, 52);
        group.apply(&mut overlay).unwrap();
        let page = overlay.get_object(3).unwrap();
        assert!(String::from_utf8_lossy(page).contains("50 0 R"));
        assert!(overlay.get_object(50).is_some(), "content object written");
        assert!(overlay.get_object(51).is_some(), "font object written");

        group.undo(&mut overlay).unwrap();
        let restored = overlay.get_object(3).unwrap();
        assert_eq!(restored, PAGE_NO_RESOURCES);
    }

    #[test]
    fn build_group_skips_font_object_write_when_f1_already_present() {
        let page_with_f1 = b"3 0 obj\n<< /Type /Page /Contents 4 0 R /Resources << /Font << /F1 9 0 R >> >> >>\nendobj\n";
        let (group, next) = build_apply_ocr_text_layer_group(
            0,
            3,
            page_with_f1.to_vec(),
            b"BT ET",
            50,
        )
        .unwrap();
        // Object numbers are still reserved (simpler, harmless)...
        assert_eq!(next, 52);
        // ...but only the content object is actually written for apply/undo.
        let mut overlay = CowOverlay::new();
        group.apply(&mut overlay).unwrap();
        assert!(overlay.get_object(50).is_some(), "content object written");
        assert!(
            overlay.get_object(51).is_none(),
            "font object not written when /F1 already existed"
        );
    }
}
