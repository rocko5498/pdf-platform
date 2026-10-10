//! The revision history a document carries. [FR-VER-1, ADR-012]
//!
//! Saving is incremental: each save appends objects and a new cross-reference
//! section instead of rewriting the file, so everything earlier stays in the
//! bytes. That is the point of the design (PRIN-2, untouched bytes and valid
//! signatures survive) and it is also a privacy fact: a page deleted or text
//! changed in revision 3 is still readable in revision 1. This lists them.
//!
//! A linearized file has two cross-reference sections that are *one* save (the
//! first-page section and the main section, ISO 32000-1 Annex F). Counting
//! them as two would tell a user their freshly optimized file had been edited.

use crate::scan::{find_startxref, is_linearized, next_section, parse_section, ScanError};

/// A document with more saves than this is either pathological or hostile.
const MAX_SECTIONS: usize = 64;

/// One saved state of the document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Revision {
    /// 1 is the original save; higher numbers are later incremental saves.
    pub number: u32,
    /// Offset of this revision's cross-reference section (the later of the two
    /// for a linearized original).
    pub xref_offset: usize,
    /// Offset just past the `%%EOF` that closes the revision.
    pub end_offset: usize,
    /// Bytes this revision added to the file.
    pub bytes: usize,
    /// Objects this revision's section lists as in use: written here for the
    /// first time or rewritten.
    pub objects_written: usize,
}

/// The revisions of `data`, oldest first.
///
/// Fails when the newest cross-reference section cannot be read; an older
/// section that cannot be read ends the list there, since nothing before it
/// can be located.
pub fn list_revisions(data: &[u8]) -> Result<Vec<Revision>, ScanError> {
    let start = find_startxref(data).ok_or(ScanError::NoStartxref)?;
    let mut leniency = Vec::new();

    // Newest first: (section offset, objects it lists, joined to the older one).
    let mut sections: Vec<(usize, usize, bool)> = Vec::new();
    let mut visited: Vec<usize> = Vec::new();
    let mut current = start;
    loop {
        if visited.contains(&current) || visited.len() >= MAX_SECTIONS {
            break;
        }
        visited.push(current);

        let entries = match parse_section(data, current, &mut leniency) {
            Ok(entries) => entries,
            Err(error) if visited.len() == 1 => return Err(error),
            Err(_) => break,
        };
        let written = entries
            .iter()
            .filter(|e| e.in_use && (e.offset != 0 || e.compressed.is_some()))
            .count();

        let next = next_section(data, current, &mut leniency);
        sections.push((current, written, next.is_some_and(|(_, forward)| forward)));
        match next {
            Some((offset, _)) => current = offset,
            None => break,
        }
    }

    // Oldest first, folding a linearized first-page section into the main
    // section it points forward to.
    let mut revisions: Vec<Revision> = Vec::new();
    let mut previous_end = 0usize;
    for &(offset, written, joined_to_older) in sections.iter().rev() {
        if joined_to_older {
            if let Some(last) = revisions.last_mut() {
                last.objects_written += written;
                last.xref_offset = last.xref_offset.max(offset);
                let end = end_of_revision(data, last.xref_offset);
                last.bytes = end - (previous_end - last.bytes).min(end);
                last.end_offset = end;
                previous_end = end;
                continue;
            }
        }
        let end = end_of_revision(data, offset);
        revisions.push(Revision {
            number: revisions.len() as u32 + 1,
            xref_offset: offset,
            end_offset: end,
            bytes: end.saturating_sub(previous_end),
            objects_written: written,
        });
        previous_end = end;
    }
    debug_assert!(!is_linearized(data) || !revisions.is_empty());
    Ok(revisions)
}

/// Offset just past the first `%%EOF` at or after `from`, and the end-of-line
/// after it; the end of the file when there is none.
fn end_of_revision(data: &[u8], from: usize) -> usize {
    const MARKER: &[u8] = b"%%EOF";
    let tail = match data.get(from..) {
        Some(tail) => tail,
        None => return data.len(),
    };
    let Some(at) = tail.windows(MARKER.len()).position(|w| w == MARKER) else {
        return data.len();
    };
    let mut end = from + at + MARKER.len();
    if data.get(end) == Some(&b'\r') {
        end += 1;
    }
    if data.get(end) == Some(&b'\n') {
        end += 1;
    }
    end
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    /// A document saved `n` times: revision 1 writes the catalog and page tree,
    /// each later one appends one object and a section pointing back with
    /// `/Prev`. With `loop_last`, the newest section's `/Prev` names itself.
    fn document_with_revisions(n: usize, loop_last: bool) -> Vec<u8> {
        let mut bytes: Vec<u8> = b"%PDF-1.7\n".to_vec();
        let mut previous_xref: Option<usize> = None;
        for revision in 0..n {
            let xref;
            let size;
            if revision == 0 {
                let catalog = bytes.len();
                write!(bytes, "1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n").unwrap();
                let pages = bytes.len();
                write!(bytes, "2 0 obj\n<< /Type /Pages /Kids [] /Count 0 >>\nendobj\n").unwrap();
                xref = bytes.len();
                write!(
                    bytes,
                    "xref\n0 3\n0000000000 65535 f \n{catalog:010} 00000 n \n{pages:010} 00000 n \n"
                )
                .unwrap();
                size = 3;
            } else {
                let number = revision + 2;
                let at = bytes.len();
                write!(bytes, "{number} 0 obj\n<< /Revision {revision} >>\nendobj\n").unwrap();
                xref = bytes.len();
                write!(bytes, "xref\n{number} 1\n{at:010} 00000 n \n").unwrap();
                size = number + 1;
            }
            let prev = if loop_last && revision == n - 1 {
                Some(xref)
            } else {
                previous_xref
            };
            let prev = prev.map(|p| format!(" /Prev {p}")).unwrap_or_default();
            write!(
                bytes,
                "trailer\n<< /Size {size} /Root 1 0 R{prev} >>\nstartxref\n{xref}\n%%EOF\n"
            )
            .unwrap();
            previous_xref = Some(xref);
        }
        bytes
    }
    #[test]
    fn a_document_saved_once_has_one_revision() {
        let data = document_with_revisions(1, false);
        let revisions = list_revisions(&data).expect("list");
        assert_eq!(revisions.len(), 1);
        assert_eq!(revisions[0].number, 1);
        assert_eq!(revisions[0].end_offset, data.len(), "the only revision ends at the end of the file");
        assert_eq!(revisions[0].bytes, data.len());
        assert_eq!(revisions[0].objects_written, 2, "the catalog and the page tree");
    }

    #[test]
    fn each_incremental_save_is_one_more_revision_oldest_first() {
        let data = document_with_revisions(3, false);
        let revisions = list_revisions(&data).expect("list");
        assert_eq!(revisions.iter().map(|r| r.number).collect::<Vec<_>>(), [1, 2, 3]);
        assert_eq!(
            revisions.iter().map(|r| r.objects_written).collect::<Vec<_>>(),
            [2, 1, 1],
            "the original wrote two objects, each update one"
        );
        // The revisions tile the file: no byte is counted twice or missed.
        assert_eq!(revisions.iter().map(|r| r.bytes).sum::<usize>(), data.len());
        assert_eq!(revisions.last().unwrap().end_offset, data.len());
        assert!(revisions.windows(2).all(|w| w[0].end_offset < w[1].end_offset));
    }

    #[test]
    fn a_file_with_no_startxref_is_an_error_not_an_empty_history() {
        assert!(list_revisions(b"%PDF-1.4\nnot much here\n").is_err());
    }

    #[test]
    fn a_self_referential_prev_does_not_spin() {
        let data = document_with_revisions(2, true);
        let revisions = list_revisions(&data).expect("the newest section is readable");
        assert_eq!(revisions.len(), 1, "a loop ends the history at the section that loops");
    }}
