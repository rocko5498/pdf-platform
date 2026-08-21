//! Page-dictionary patching for annotation attachment. [FR-ANNOT-4, SDS §3.3]
//!
//! Surgical string-level edits to page object bytes so new annotation
//! object numbers can be linked via `/Annots` without a full rewrite.

#![allow(missing_docs)]

/// Inject (or extend) a page dictionary's `/Annots` array with the given
/// object numbers. Returns a complete `N 0 obj ... endobj` buffer.
///
/// If `/Annots` already exists, new refs are prepended. If not, `/Annots`
/// is inserted before the closing `>>` of the page dictionary.
pub fn inject_annot_refs(page_obj_bytes: &[u8], annot_obj_nums: &[u32]) -> Result<Vec<u8>, String> {
    if annot_obj_nums.is_empty() {
        return Ok(page_obj_bytes.to_vec());
    }

    let text = String::from_utf8_lossy(page_obj_bytes);
    let refs: String = annot_obj_nums
        .iter()
        .map(|n| format!("{n} 0 R"))
        .collect::<Vec<_>>()
        .join(" ");

    let patched = if let Some(idx) = text.find("/Annots") {
        // Find the array after /Annots
        let after = &text[idx + 7..];
        let arr_start = after
            .find('[')
            .ok_or_else(|| "page has /Annots but no array".to_string())?;
        let abs_start = idx + 7 + arr_start;
        let after_br = &text[abs_start + 1..];
        let arr_end = after_br
            .find(']')
            .ok_or_else(|| "unclosed /Annots array".to_string())?;
        let abs_end = abs_start + 1 + arr_end;
        let existing = text[abs_start + 1..abs_end].trim();
        let new_arr = if existing.is_empty() {
            format!("[{refs}]")
        } else {
            format!("[{refs} {existing}]")
        };
        format!("{}{}{}", &text[..abs_start], new_arr, &text[abs_end + 1..])
    } else {
        // Insert before the last `>>` of the dictionary.
        let insert_at = text
            .rfind(">>")
            .ok_or_else(|| "page object missing closing >>".to_string())?;
        format!(
            "{}/Annots [{}] {}",
            &text[..insert_at],
            refs,
            &text[insert_at..]
        )
    };

    Ok(patched.into_bytes())
}

/// Append a content-stream reference and expose a font in a page's direct
/// `/Resources` dictionary. Indirect resources are rejected because mutating
/// their shared object could affect other pages. [FR-BATCH-1, ADR-013]
pub fn inject_content_ref_and_font(
    page_obj_bytes: &[u8],
    content_obj_num: u32,
    font_obj_num: u32,
    font_name: &str,
) -> Result<Vec<u8>, String> {
    if font_name.is_empty() || !font_name.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err("font resource name must be ASCII alphanumeric".into());
    }

    let mut text = String::from_utf8(page_obj_bytes.to_vec())
        .map_err(|_| "page object is not UTF-8-compatible PDF syntax".to_string())?;

    let resources = text
        .find("/Resources")
        .ok_or_else(|| "page has no explicit /Resources dictionary".to_string())?;
    let resource_value = resources + "/Resources".len();
    let value_start = resource_value
        + text[resource_value..]
            .find(|c: char| !c.is_ascii_whitespace())
            .ok_or_else(|| "page has empty /Resources".to_string())?;
    if !text[value_start..].starts_with("<<") {
        return Err("indirect /Resources is not supported for stamping".into());
    }
    let resource_end = matching_dict_end(&text, value_start)
        .ok_or_else(|| "unclosed /Resources dictionary".to_string())?;
    let resource_dict = &text[value_start..resource_end];
    let font_entry = format!("/{font_name} {font_obj_num} 0 R");
    let patched_resources = if let Some(font_pos) = resource_dict.find("/Font") {
        let font_value = font_pos + "/Font".len();
        let font_start = font_value
            + resource_dict[font_value..]
                .find(|c: char| !c.is_ascii_whitespace())
                .ok_or_else(|| "empty /Font resource".to_string())?;
        if !resource_dict[font_start..].starts_with("<<") {
            return Err("indirect /Font resources are not supported for stamping".into());
        }
        let font_end = matching_dict_end(resource_dict, font_start)
            .ok_or_else(|| "unclosed /Font dictionary".to_string())?;
        format!(
            "{} {font_entry} {}",
            &resource_dict[..font_end - 2],
            &resource_dict[font_end - 2..]
        )
    } else {
        format!(
            "{} /Font << {font_entry} >> {}",
            &resource_dict[..resource_dict.len() - 2],
            &resource_dict[resource_dict.len() - 2..]
        )
    };
    text.replace_range(value_start..resource_end, &patched_resources);

    let content_ref = format!("{content_obj_num} 0 R");
    if let Some(contents) = text.find("/Contents") {
        let value = contents + "/Contents".len();
        let start = value
            + text[value..]
                .find(|c: char| !c.is_ascii_whitespace())
                .ok_or_else(|| "page has empty /Contents".to_string())?;
        if text[start..].starts_with('[') {
            let end = start
                + text[start..]
                    .find(']')
                    .ok_or_else(|| "unclosed /Contents array".to_string())?;
            text.insert_str(end, &format!(" {content_ref}"));
        } else {
            let tokens: Vec<&str> = text[start..].split_whitespace().take(3).collect();
            if tokens.len() != 3 || tokens[2] != "R" {
                return Err("unsupported /Contents value".into());
            }
            let old_ref = tokens.join(" ");
            text.replace_range(start..start + old_ref.len(), &format!("[{old_ref} {content_ref}]"));
        }
    } else {
        let insert_at = text
            .rfind(">>")
            .ok_or_else(|| "page object missing closing >>".to_string())?;
        text.insert_str(insert_at, &format!("/Contents {content_ref} "));
    }

    Ok(text.into_bytes())
}

fn matching_dict_end(text: &str, start: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut i = start;
    while i + 1 < bytes.len() {
        match &bytes[i..i + 2] {
            b"<<" => {
                depth += 1;
                i += 2;
            }
            b">>" => {
                depth = depth.checked_sub(1)?;
                i += 2;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => i += 1,
        }
    }
    None
}

/// Extract the object number from a serialized object header (`N 0 obj`).
pub fn parse_obj_num(obj_bytes: &[u8]) -> Option<u32> {
    let text = std::str::from_utf8(obj_bytes).ok()?;
    let first = text.lines().next()?;
    first.split_whitespace().next()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamp_appends_content_and_font_to_direct_resources() {
        let page = b"3 0 obj\n<< /Type /Page /Resources << /ProcSet [/PDF /Text] >> /Contents 4 0 R >>\nendobj\n";
        let out = inject_content_ref_and_font(page, 20, 21, "FStamp").unwrap();
        let text = String::from_utf8_lossy(&out);
        assert!(text.contains("/Contents [4 0 R 20 0 R]"), "got: {text}");
        assert!(text.contains("/Font << /FStamp 21 0 R >>"), "got: {text}");
        assert!(text.contains("/ProcSet [/PDF /Text]"), "got: {text}");
    }

    #[test]
    fn stamp_appends_to_contents_array() {
        let page = b"3 0 obj\n<< /Type /Page /Resources << >> /Contents [4 0 R 5 0 R] >>\nendobj\n";
        let out = inject_content_ref_and_font(page, 20, 21, "FStamp").unwrap();
        let text = String::from_utf8_lossy(&out);
        assert!(text.contains("/Contents [4 0 R 5 0 R 20 0 R]"), "got: {text}");
    }

    #[test]
    fn stamp_rejects_indirect_resources() {
        let page = b"3 0 obj\n<< /Type /Page /Resources 8 0 R /Contents 4 0 R >>\nendobj\n";
        let error = inject_content_ref_and_font(page, 20, 21, "FStamp").unwrap_err();
        assert!(error.contains("indirect /Resources"), "got: {error}");
    }

    #[test]
    fn inject_into_page_without_annots() {
        let page = b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R >>\nendobj\n";
        let out = inject_annot_refs(page, &[50, 51]).unwrap();
        let s = String::from_utf8_lossy(&out);
        assert!(s.contains("/Annots [50 0 R 51 0 R]"), "got: {s}");
        assert!(s.contains("/Type /Page"));
    }

    #[test]
    fn inject_extends_existing_annots() {
        let page = b"3 0 obj\n<< /Type /Page /Annots [10 0 R] /MediaBox [0 0 612 792] >>\nendobj\n";
        let out = inject_annot_refs(page, &[50]).unwrap();
        let s = String::from_utf8_lossy(&out);
        assert!(s.contains("50 0 R"));
        assert!(s.contains("10 0 R"));
    }
}
