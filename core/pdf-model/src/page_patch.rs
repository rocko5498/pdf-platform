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
