//! Incremental-append + full-rewrite serializers. [ADR-012, SDS §3.4]
//!
//! Default save = incremental append: serialize the CoW overlay as new
//! object versions + xref section + trailer with /Prev. Untouched bytes
//! are never rewritten. [ADR-012 §1]
//!
//! Full rewrite ("Optimize/Save As Clean") is a distinct, explicit operation
//! that linearizes, repacks, and garbage-collects — always preceded by a
//! pre-flight report. [ADR-012 §3]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::collections::HashMap;
use std::io::Write;

use pdf_model::overlay::CowOverlay;

/// Write result: what was written and where.
#[derive(Debug)]
pub struct WriteResult {
    /// Number of object versions written.
    pub objects_written: u32,
    /// Byte offset where the new xref section starts.
    pub xref_offset: u32,
    /// Total bytes appended.
    pub bytes_appended: u32,
}

/// Incremental-save serializer. [ADR-012]
///
/// Appends changed objects and a new xref section to the existing file.
/// The original file bytes are never modified — only new bytes are appended.
pub struct IncrementalWriter;

impl IncrementalWriter {
    /// Serialize the CoW overlay as an incremental update.
    ///
    /// `writer` is the output (typically the end of the original file).
    /// `overlay` contains the changed objects.
    /// `prev_xref_offset` is the byte offset of the previous xref section.
    /// `next_obj_num` is the next available object number (for new objects).
    /// `original_offsets` maps object numbers to their byte offsets in the
    /// original file, parsed from the original xref table by the worker.
    /// `original_len` is the byte length of the original file content
    /// already written to `writer` (new bytes are appended after this).
    ///
    /// Returns a `WriteResult` with the new xref offset.
    pub fn write_incremental(
        writer: &mut impl Write,
        overlay: &CowOverlay,
        prev_xref_offset: u32,
        next_obj_num: u32,
        original_offsets: &HashMap<u32, u32>,
        original_len: u32,
    ) -> Result<WriteResult, std::io::Error> {
        let mut objects_written = 0u32;

        // Track byte positions of dirty objects as we write them.
        // Positions start after the original file content.
        let mut dirty_offsets: HashMap<u32, u32> = HashMap::new();
        let mut current_pos = original_len;

        // First pass: write all modified objects and record their offsets.
        let mut object_buffers: Vec<(u32, Vec<u8>)> = Vec::new();
        for obj_num in 1..next_obj_num {
            if let Some(bytes) = overlay.get_object(obj_num) {
                object_buffers.push((obj_num, bytes.to_vec()));
            }
        }

        // Sort by object number for deterministic output.
        object_buffers.sort_by_key(|(n, _)| *n);

        for (obj_num, bytes) in &object_buffers {
            dirty_offsets.insert(*obj_num, current_pos);
            current_pos += bytes.len() as u32;
            objects_written += 1;
        }

        // Second pass: write objects to the actual writer.
        for (obj_num, bytes) in &object_buffers {
            writer.write_all(bytes)?;
        }

        // Build xref section.
        let xref_start = current_pos;
        let max_obj = next_obj_num.max(
            original_offsets.keys().copied().max().unwrap_or(0).max(
                overlay.dirty_objects().iter().copied().max().unwrap_or(0)
            ) + 1
        );

        let xref_header = format!("xref\n0 {max_obj}\n");

        // Write xref header.
        write!(writer, "{xref_header}")?;
        current_pos += xref_header.len() as u32;

        // Write xref entries: for each object, use dirty offset if modified,
        // otherwise use the original offset from the parsed xref table.
        for obj_num in 0..max_obj {
            let offset = if let Some(&off) = dirty_offsets.get(&obj_num) {
                off
            } else if let Some(&off) = original_offsets.get(&obj_num) {
                off
            } else {
                0 // Free object or unknown
            };

            let in_use = dirty_offsets.contains_key(&obj_num)
                || original_offsets.contains_key(&obj_num);

            let entry = if in_use && obj_num > 0 {
                format!("{offset:010} 00000 n \n")
            } else {
                format!("{offset:010} 65535 f \n")
            };

            writer.write_all(entry.as_bytes())?;
            current_pos += entry.len() as u32;
        }

        // Write trailer.
        //
        // `/Prev 0` is not "no previous section", it is a pointer at byte 0. A
        // reader that follows it lands on the file header rather than an xref,
        // so the incremental update is malformed. Omit the key entirely when
        // there is nothing to chain to. [SDS §3.3, PRIN-1, GR-8]
        let trailer = if prev_xref_offset > 0 {
            format!("trailer\n<< /Size {max_obj} /Root 1 0 R /Prev {prev_xref_offset} >>\n")
        } else {
            format!("trailer\n<< /Size {max_obj} /Root 1 0 R >>\n")
        };
        writer.write_all(trailer.as_bytes())?;
        current_pos += trailer.len() as u32;

        let startxref = format!("startxref\n{xref_start}\n%%EOF\n");
        writer.write_all(startxref.as_bytes())?;
        current_pos += startxref.len() as u32;

        Ok(WriteResult {
            objects_written,
            xref_offset: xref_start,
            bytes_appended: current_pos,
        })
    }

    /// Estimate the size of an incremental save for the given overlay.
    ///
    /// Useful for pre-flight reports and progress estimation.
    pub fn estimate_size(overlay: &CowOverlay, known_objects: u32) -> u32 {
        let mut size = 0u32;

        // Object bytes.
        for &obj_num in overlay.dirty_objects() {
            if let Some(bytes) = overlay.get_object(obj_num) {
                size += bytes.len() as u32;
            }
        }

        // xref: header + per-object entry (20 bytes each) + trailer (~100 bytes).
        size += 6; // "xref\n"
        size += format!("0 {}\n", known_objects + 1).len() as u32;
        size += 20 * (known_objects + 1); // 20 bytes per entry
        size += 100; // trailer + startxref + %%EOF

        size
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn incremental_write_produces_valid_structure() {
        let mut overlay = CowOverlay::new();
        overlay.set_object(1, b"1 0 obj\n<< /Type /Catalog >>\nendobj\n".to_vec());
        overlay.set_object(3, b"3 0 obj\n<< /Type /Page >>\nendobj\n".to_vec());

        let mut offsets = HashMap::new();
        offsets.insert(2, 100); // non-dirty object has original offset

        let mut output = Vec::new();
        let result = IncrementalWriter::write_incremental(
            &mut output,
            &overlay,
            100, // prev xref at offset 100
            5,   // next obj num is 5
            &offsets,
            0,   // original_len: writing to empty buffer
        ).unwrap();

        assert!(result.objects_written >= 2);
        let text = String::from_utf8_lossy(&output);
        assert!(text.contains("xref"));
        assert!(text.contains("trailer"));
        assert!(text.contains("/Prev 100"));
        assert!(text.contains("%%EOF"));
    }

    #[test]
    fn incremental_write_uses_original_offsets_for_non_dirty() {
        let mut overlay = CowOverlay::new();
        overlay.set_object(1, b"1 0 obj\n<< /Type /Catalog >>\nendobj\n".to_vec());

        let mut offsets = HashMap::new();
        offsets.insert(2, 200); // obj 2 not dirty, has original offset at 200
        offsets.insert(3, 350); // obj 3 not dirty, has original offset at 350

        let mut output = Vec::new();
        let result = IncrementalWriter::write_incremental(
            &mut output,
            &overlay,
            100,
            5,
            &offsets,
            0,
        ).unwrap();

        let text = String::from_utf8_lossy(&output);
        // Dirty obj 1 should have offset 0 (start of append)
        assert!(text.contains("0000000000 00000 n"));
        // Non-dirty obj 2 should have original offset 200
        assert!(text.contains("0000000200 00000 n"));
        // Non-dirty obj 3 should have original offset 350
        assert!(text.contains("0000000350 00000 n"));
    }

    #[test]
    fn estimate_size_accounts_for_objects() {
        let mut overlay = CowOverlay::new();
        overlay.set_object(1, b"1 0 obj\n<< /Type /Catalog >>\nendobj\n".to_vec());

        let size = IncrementalWriter::estimate_size(&overlay, 4);
        // Should include object bytes + xref + trailer.
        assert!(size > 100);
    }

    #[test]
    fn incremental_write_empty_overlay() {
        let overlay = CowOverlay::new();
        let mut output = Vec::new();
        let result = IncrementalWriter::write_incremental(
            &mut output,
            &overlay,
            0,
            3,
            &HashMap::new(),
            0,
        ).unwrap();

        assert_eq!(result.objects_written, 0);
        let text = String::from_utf8_lossy(&output);
        assert!(text.contains("xref"));
        assert!(text.contains("%%EOF"));
    }

    #[test]
    fn incremental_write_xref_offset_is_correct() {
        let mut overlay = CowOverlay::new();
        overlay.set_object(1, b"1 0 obj\n<< /Type /Catalog >>\nendobj\n".to_vec());

        let mut output = Vec::new();
        let result = IncrementalWriter::write_incremental(
            &mut output,
            &overlay,
            0,
            2,
            &HashMap::new(),
            0,
        ).unwrap();

        // The xref_offset should equal the byte position of "xref\n" in the output.
        let text = String::from_utf8_lossy(&output);
        let xref_pos = text.find("xref\n").unwrap() as u32;
        assert_eq!(result.xref_offset, xref_pos);

        // startxref should reference the correct position.
        let startxref_line = format!("startxref\n{xref_pos}\n%%EOF");
        assert!(text.contains(&startxref_line));
    }

    #[test]
    fn a_first_revision_omits_prev_rather_than_pointing_at_byte_zero() {
        let mut overlay = CowOverlay::new();
        overlay.set_object(1, b"1 0 obj
<< /Type /Catalog >>
endobj
".to_vec());

        let mut output = Vec::new();
        IncrementalWriter::write_incremental(
            &mut output,
            &overlay,
            0, // nothing to chain to
            2,
            &HashMap::new(),
            0,
        )
        .unwrap();

        let text = String::from_utf8_lossy(&output);
        assert!(text.contains("trailer"), "{text}");
        assert!(
            !text.contains("/Prev"),
            "`/Prev 0` points at the file header, not at an xref section: {text}"
        );
    }
}
