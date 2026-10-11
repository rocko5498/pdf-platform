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
/// What the new trailer must say, taken from the document being saved.
///
/// Every field here was once assumed: `/Root` was written as `1 0 R`, and
/// `/Info`, `/Encrypt` and `/ID` were dropped. [FR-SAVE, SDS §3.3]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrailerInfo {
    /// Object number of the document catalog.
    pub root_obj_num: u32,
    /// Object number of the document information dictionary, if it has one.
    pub info_obj_num: Option<u32>,
    /// Object number of the encryption dictionary, if the document is encrypted.
    pub encrypt_obj_num: Option<u32>,
    /// The `/ID` array exactly as the source wrote it, brackets included.
    pub id: Option<String>,
}

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
        trailer: &TrailerInfo,
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

        // An object whose bytes do not end in a newline runs straight into the
        // next one — `endobj4 0 obj` — which a conforming lexer is not obliged
        // to split. The bytes used to carry a trailing newline by accident,
        // because `get_object` over-read past `endobj`; once that was fixed the
        // separator had to become deliberate. [SDS §3.4, PRIN-1]
        fn needs_separator(bytes: &[u8]) -> bool {
            !bytes.ends_with(b"\n")
        }
        for (obj_num, bytes) in &object_buffers {
            dirty_offsets.insert(*obj_num, current_pos);
            current_pos += bytes.len() as u32 + u32::from(needs_separator(bytes));
            objects_written += 1;
        }

        // Second pass: write objects to the actual writer.
        for (_obj_num, bytes) in &object_buffers {
            writer.write_all(bytes)?;
            if needs_separator(bytes) {
                writer.write_all(b"\n")?;
            }
        }

        // Build the xref section: the objects this save wrote, and nothing else.
        //
        // An incremental update lists what it changed and chains to the section
        // it supersedes with `/Prev`; every untouched object stays where the
        // earlier section put it. This used to emit a *complete* table built
        // from `original_offsets`, which worked only while every object was an
        // ordinary one at a file offset. An object inside an object stream has
        // no offset, so it was written as `0000000000 00000 n` — in use, at
        // byte 0 — and a saved qpdf-style document reopened with `/Root`
        // pointing at the file header and no pages. The reader has followed
        // `/Prev` chains since 2026-08-23, so the complete table was never
        // needed. [FR-SAVE-1, ADR-012, SDS §3.3, PRIN-1, GR-8]
        let xref_start = current_pos;
        let max_obj = next_obj_num.max(
            original_offsets.keys().copied().max().unwrap_or(0).max(
                overlay.dirty_objects().iter().copied().max().unwrap_or(0)
            ) + 1
        );

        // With nothing to chain to (`prev_xref_offset == 0`: the source had no
        // usable cross-reference, so it was rebuilt by scanning), a table that
        // lists only this save's objects would leave every other object
        // unknown to the next reader. Then the table is complete: it also lists
        // the source's objects at the offsets the rebuild found. Only entries
        // with a real offset are written; an object living in an object stream
        // has none, and a rebuilt source has no such objects. [FR-SAVE, SDS §10.4]
        let mut table: std::collections::BTreeMap<u32, u32> = dirty_offsets.clone().into_iter().collect();
        if prev_xref_offset == 0 {
            for (number, offset) in original_offsets {
                if *offset > 0 {
                    table.entry(*number).or_insert(*offset);
                }
            }
        }

        let mut section = String::from("xref\n");
        // The free-list head, which an update section carries by convention.
        section.push_str("0 1\n0000000000 65535 f \n");
        // One subsection per run of consecutive object numbers.
        let numbers: Vec<u32> = table.keys().copied().collect();
        let mut at = 0;
        while at < numbers.len() {
            let mut end = at + 1;
            while end < numbers.len() && numbers[end] == numbers[end - 1] + 1 {
                end += 1;
            }
            section.push_str(&format!("{} {}\n", numbers[at], end - at));
            for number in &numbers[at..end] {
                section.push_str(&format!("{:010} 00000 n \n", table[number]));
            }
            at = end;
        }
        writer.write_all(section.as_bytes())?;
        current_pos += section.len() as u32;

        // Write trailer.
        //
        // `/Prev 0` is not "no previous section", it is a pointer at byte 0. A
        // reader that follows it lands on the file header rather than an xref,
        // so the incremental update is malformed. Omit the key entirely when
        // there is nothing to chain to. [SDS §3.3, PRIN-1, GR-8]
        // `/Root 1 0 R` used to be written literally. A document that numbers
        // its catalog anything but 1 — which the format allows and producers do
        // — was handed back with a trailer pointing at the wrong object, and
        // reopening it found no pages. Everything the source trailer said that
        // still applies is carried forward: an encrypted document whose /Encrypt
        // is dropped cannot be decrypted, and losing /ID breaks the file's
        // identity for signatures and for update chains. [FR-SAVE, SDS §3.3]
        let mut entries = format!("/Size {max_obj} /Root {} 0 R", trailer.root_obj_num);
        if let Some(info) = trailer.info_obj_num {
            entries.push_str(&format!(" /Info {info} 0 R"));
        }
        if let Some(encrypt) = trailer.encrypt_obj_num {
            entries.push_str(&format!(" /Encrypt {encrypt} 0 R"));
        }
        if let Some(id) = &trailer.id {
            entries.push_str(&format!(" /ID {id}"));
        }
        if prev_xref_offset > 0 {
            entries.push_str(&format!(" /Prev {prev_xref_offset}"));
        }
        let trailer_text = format!("trailer\n<< {entries} >>\n");
        writer.write_all(trailer_text.as_bytes())?;
        current_pos += trailer_text.len() as u32;

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
            &TrailerInfo { root_obj_num: 1, ..Default::default() },
        ).unwrap();

        assert!(result.objects_written >= 2);
        let text = String::from_utf8_lossy(&output);
        assert!(text.contains("xref"));
        assert!(text.contains("trailer"));
        assert!(text.contains("/Prev 100"));
        assert!(text.contains("%%EOF"));
    }

    /// The `xref` section of `output`, up to its trailer.
    fn xref_section(output: &[u8]) -> String {
        let text = String::from_utf8_lossy(output).into_owned();
        let start = text.find("xref\n").expect("an xref section");
        let end = text[start..].find("trailer").expect("a trailer") + start;
        text[start..end].to_string()
    }

    #[test]
    fn an_update_lists_only_the_objects_it_wrote() {
        let mut overlay = CowOverlay::new();
        overlay.set_object(1, b"1 0 obj\n<< /Type /Catalog >>\nendobj\n".to_vec());

        let mut offsets = HashMap::new();
        offsets.insert(2, 200); // untouched: the previous section still owns it
        offsets.insert(3, 350);

        let mut output = Vec::new();
        IncrementalWriter::write_incremental(
            &mut output, &overlay, 100, 5, &offsets, 0,
            &TrailerInfo { root_obj_num: 1, ..Default::default() },
        )
        .unwrap();

        let section = xref_section(&output);
        assert_eq!(
            section,
            "xref\n0 1\n0000000000 65535 f \n1 1\n0000000000 00000 n \n",
            "the free-list head and the one object this save wrote, nothing else"
        );
        assert!(!section.contains("0000000200"), "an untouched object was repeated: {section}");
        assert!(!section.contains("0000000350"), "an untouched object was repeated: {section}");
        let text = String::from_utf8_lossy(&output);
        assert!(text.contains("/Prev 100"), "the untouched objects are reached through /Prev");
        assert!(text.contains("/Size 5"), "/Size still covers every object number");
    }

    #[test]
    fn objects_that_are_not_consecutive_get_their_own_subsections() {
        let mut overlay = CowOverlay::new();
        for n in [1u32, 2, 5, 9] {
            overlay.set_object(n, format!("{n} 0 obj\n<< >>\nendobj\n").into_bytes());
        }
        let mut output = Vec::new();
        IncrementalWriter::write_incremental(
            &mut output, &overlay, 100, 10, &HashMap::new(), 0,
            &TrailerInfo { root_obj_num: 1, ..Default::default() },
        )
        .unwrap();

        let section = xref_section(&output);
        let headers: Vec<&str> = section.lines().filter(|l| l.split(' ').count() == 2).collect();
        assert_eq!(headers, ["0 1", "1 2", "5 1", "9 1"], "runs of consecutive numbers: {section}");
        let entries = section.lines().filter(|l| l.ends_with(" n ")).count();
        assert_eq!(entries, 4, "one in-use entry per object written: {section}");
    }

    #[test]
    fn an_original_object_with_no_file_offset_is_never_written_as_offset_zero() {
        // An object inside an object stream is "in use" with no offset of its
        // own; the worker records it as offset 0. Writing that into a classic
        // table puts the object at the file header. [GR-8, PRIN-1]
        let mut overlay = CowOverlay::new();
        overlay.set_object(3, b"3 0 obj\n<< /Type /Page >>\nendobj\n".to_vec());
        let mut offsets = HashMap::new();
        offsets.insert(1, 0); // the catalog, compressed in an object stream
        offsets.insert(2, 0); // the page tree, likewise

        let mut output = Vec::new();
        IncrementalWriter::write_incremental(
            &mut output, &overlay, 100, 4, &offsets, 500,
            &TrailerInfo { root_obj_num: 1, ..Default::default() },
        )
        .unwrap();

        let section = xref_section(&output);
        let in_use_at_zero = section.lines().filter(|l| *l == "0000000000 00000 n ").count();
        assert_eq!(in_use_at_zero, 0, "an in-use entry at byte 0: {section}");
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
            &TrailerInfo { root_obj_num: 1, ..Default::default() },
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
            &TrailerInfo { root_obj_num: 1, ..Default::default() },
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
            &TrailerInfo { root_obj_num: 1, ..Default::default() },
        )
        .unwrap();

        let text = String::from_utf8_lossy(&output);
        assert!(text.contains("trailer"), "{text}");
        assert!(
            !text.contains("/Prev"),
            "`/Prev 0` points at the file header, not at an xref section: {text}"
        );
    }

    /// With no previous section to chain to, the table must be complete: it
    /// also lists the source's objects, or the saved file knows only this save's
    /// objects. Found by the corpus edit sweep on truncated files whose
    /// `startxref` pointed past the end. [FR-SAVE, SDS §10.4]
    #[test]
    fn with_no_previous_section_the_table_lists_the_source_objects_too() {
        let mut overlay = CowOverlay::new();
        overlay.set_object(5, b"5 0 obj\n<< /Type /Pages >>\nendobj\n".to_vec());
        let offsets: HashMap<u32, u32> = [(1, 9), (2, 60), (3, 120)].into_iter().collect();

        let mut chained = Vec::new();
        IncrementalWriter::write_incremental(&mut chained, &overlay, 77, 6, &offsets, 400, &TrailerInfo { root_obj_num: 1, ..Default::default() }).unwrap();
        let section = xref_section(&chained);
        assert!(section.contains("5 1\n"), "{section}");
        assert!(!section.contains("0000000060"), "a chained update lists only what it wrote: {section}");

        let mut complete = Vec::new();
        IncrementalWriter::write_incremental(&mut complete, &overlay, 0, 6, &offsets, 400, &TrailerInfo { root_obj_num: 1, ..Default::default() }).unwrap();
        let section = xref_section(&complete);
        for offset in ["0000000009", "0000000060", "0000000120"] {
            assert!(section.contains(offset), "{offset} missing: {section}");
        }
        assert!(!String::from_utf8_lossy(&complete).contains("/Prev"));
    }
}
