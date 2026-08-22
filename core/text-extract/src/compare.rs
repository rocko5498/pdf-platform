//! Reflow-resilient line comparison for document diff. [FR-CMP-1, FR-CMP-2, FR-CMP-3]
//!
//! `FR-CMP-3` requires textual comparison to be "resilient to reflow and
//! pagination changes to the extent feasible, prioritizing meaningful change
//! detection over raw positional diff". Pairing lines by index fails that
//! outright: inserting a single line at the top reports every following line
//! as changed, which is the "raw positional diff" the requirement rules out.
//!
//! This computes a longest-common-subsequence alignment instead, so an
//! insertion or deletion costs one operation rather than shifting everything
//! after it.

/// One aligned line in a comparison.
///
/// FR-CMP-2 requires differences to be presented "navigably, indicating
/// locations and the nature of each change (added, removed, changed, moved
/// where detectable)", so every variant carries the line index it refers to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineDiff {
    /// Present in both sides, unchanged.
    Same {
        /// The line text.
        text: String,
        /// Index in the earlier document.
        before_index: usize,
        /// Index in the later document.
        after_index: usize,
    },
    /// Present only in the earlier document.
    Removed {
        /// The line text.
        text: String,
        /// Index in the earlier document.
        before_index: usize,
    },
    /// Present only in the later document.
    Added {
        /// The line text.
        text: String,
        /// Index in the later document.
        after_index: usize,
    },
    /// The same line, at a different position in each document.
    ///
    /// "Moved where detectable" (FR-CMP-2) is precisely that qualifier: a move
    /// is detected by pairing a removal with an identical addition elsewhere,
    /// so this catches a relocated line, never a relocated-and-edited one.
    Moved {
        /// The line text, identical on both sides.
        text: String,
        /// Index in the earlier document.
        before_index: usize,
        /// Index in the later document.
        after_index: usize,
    },
}

/// Maximum lines per side for the LCS alignment. [GR-7]
///
/// The dynamic-programming table is `(before+1) * (after+1)` `u32` cells, so
/// the bound caps it at roughly 16 MB. Beyond this the comparison degrades to
/// positional pairing rather than allocating without limit; `diff_lines`
/// reports which path it took via [`DiffQuality`].
pub const MAX_ALIGNED_LINES: usize = 2000;

/// Whether a comparison was aligned or fell back to positional pairing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffQuality {
    /// Lines were aligned by longest common subsequence.
    Aligned,
    /// One side exceeded [`MAX_ALIGNED_LINES`]; lines were paired by index and
    /// the result is not reflow-resilient.
    PositionalFallback,
}

/// Align two sequences of lines, tolerating insertions and deletions.
///
/// Returns the aligned operations and whether alignment was actually
/// performed — a caller that reports "identical" or counts changes must not
/// present a [`DiffQuality::PositionalFallback`] result as a reflow-resilient
/// comparison. [FR-CMP-3, PRIN-6]
pub fn diff_lines(before: &[&str], after: &[&str]) -> (Vec<LineDiff>, DiffQuality) {
    if before.len() > MAX_ALIGNED_LINES || after.len() > MAX_ALIGNED_LINES {
        return (
            detect_moves(positional(before, after)),
            DiffQuality::PositionalFallback,
        );
    }
    (detect_moves(aligned(before, after)), DiffQuality::Aligned)
}

/// Rewrite removal/addition pairs of identical text as moves. [FR-CMP-2]
///
/// A relocated paragraph otherwise reads as an unrelated deletion plus an
/// unrelated insertion: two changes to review instead of one, and the fact
/// that the text itself is untouched is lost — which is the distinction a
/// legal redline exists to show (US-LAW-3).
///
/// Pairing is first-come in document order and only between byte-identical
/// lines. A line that moved *and* changed stays a removal plus an addition,
/// because reporting it as a move would claim tracking that was not done.
/// Repeated identical lines pair in order, so N removals and M additions of
/// the same text yield min(N, M) moves.
fn detect_moves(ops: Vec<LineDiff>) -> Vec<LineDiff> {
    use std::collections::HashMap;

    let mut pending: HashMap<&str, Vec<usize>> = HashMap::new();
    for (position, op) in ops.iter().enumerate() {
        if let LineDiff::Added { text, .. } = op {
            pending.entry(text.as_str()).or_default().push(position);
        }
    }

    let mut consumed = vec![false; ops.len()];
    let mut paired: HashMap<usize, usize> = HashMap::new();
    for (position, op) in ops.iter().enumerate() {
        let LineDiff::Removed { text, .. } = op else {
            continue;
        };
        let Some(candidates) = pending.get_mut(text.as_str()) else {
            continue;
        };
        if let Some(addition) = candidates.iter().copied().find(|c| !consumed[*c]) {
            consumed[addition] = true;
            paired.insert(position, addition);
        }
    }

    if paired.is_empty() {
        return ops;
    }

    let mut out: Vec<LineDiff> = Vec::with_capacity(ops.len());
    for (position, op) in ops.iter().enumerate() {
        match op {
            LineDiff::Removed { text, before_index } if paired.contains_key(&position) => {
                let addition = paired[&position];
                let LineDiff::Added { after_index, .. } = &ops[addition] else {
                    unreachable!("only additions are ever paired");
                };
                out.push(LineDiff::Moved {
                    text: text.clone(),
                    before_index: *before_index,
                    after_index: *after_index,
                });
            }
            // The addition half of a pair is already reported as the move.
            LineDiff::Added { .. } if consumed[position] => {}
            other => out.push(other.clone()),
        }
    }
    out
}

fn aligned(before: &[&str], after: &[&str]) -> Vec<LineDiff> {
    let (rows, cols) = (before.len(), after.len());

    // lengths[i][j] = LCS length of before[i..] and after[j..], built from the
    // end so the backtrack below can walk forward and emit in document order.
    let stride = cols + 1;
    let mut lengths = vec![0u32; (rows + 1) * stride];
    for i in (0..rows).rev() {
        for j in (0..cols).rev() {
            lengths[i * stride + j] = if before[i] == after[j] {
                lengths[(i + 1) * stride + j + 1] + 1
            } else {
                lengths[(i + 1) * stride + j].max(lengths[i * stride + j + 1])
            };
        }
    }

    let mut ops = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < rows && j < cols {
        if before[i] == after[j] {
            ops.push(LineDiff::Same {
                text: before[i].to_owned(),
                before_index: i,
                after_index: j,
            });
            i += 1;
            j += 1;
        } else if lengths[(i + 1) * stride + j] >= lengths[i * stride + j + 1] {
            ops.push(LineDiff::Removed {
                text: before[i].to_owned(),
                before_index: i,
            });
            i += 1;
        } else {
            ops.push(LineDiff::Added {
                text: after[j].to_owned(),
                after_index: j,
            });
            j += 1;
        }
    }
    let tail_before = i;
    ops.extend(before[tail_before..].iter().enumerate().map(|(offset, line)| {
        LineDiff::Removed {
            text: (*line).to_owned(),
            before_index: tail_before + offset,
        }
    }));
    let tail_after = j;
    ops.extend(after[tail_after..].iter().enumerate().map(|(offset, line)| {
        LineDiff::Added {
            text: (*line).to_owned(),
            after_index: tail_after + offset,
        }
    }));
    ops
}

fn positional(before: &[&str], after: &[&str]) -> Vec<LineDiff> {
    let mut ops = Vec::new();
    for index in 0..before.len().max(after.len()) {
        match (before.get(index), after.get(index)) {
            (Some(b), Some(a)) if b == a => ops.push(LineDiff::Same {
                text: (*b).to_owned(),
                before_index: index,
                after_index: index,
            }),
            (Some(b), Some(a)) => {
                ops.push(LineDiff::Removed {
                    text: (*b).to_owned(),
                    before_index: index,
                });
                ops.push(LineDiff::Added {
                    text: (*a).to_owned(),
                    after_index: index,
                });
            }
            (Some(b), None) => ops.push(LineDiff::Removed {
                text: (*b).to_owned(),
                before_index: index,
            }),
            (None, Some(a)) => ops.push(LineDiff::Added {
                text: (*a).to_owned(),
                after_index: index,
            }),
            (None, None) => unreachable!("index is bounded by the longer side"),
        }
    }
    ops
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ops(before: &[&str], after: &[&str]) -> Vec<LineDiff> {
        let (ops, quality) = diff_lines(before, after);
        assert_eq!(quality, DiffQuality::Aligned);
        ops
    }

    /// Compact rendering: kind, text, and the indices FR-CMP-2 asks for.
    fn shape(ops: &[LineDiff]) -> Vec<String> {
        ops.iter()
            .map(|op| match op {
                LineDiff::Same { text, before_index, after_index } => {
                    format!("same {text} {before_index}->{after_index}")
                }
                LineDiff::Removed { text, before_index } => format!("removed {text} {before_index}"),
                LineDiff::Added { text, after_index } => format!("added {text} {after_index}"),
                LineDiff::Moved { text, before_index, after_index } => {
                    format!("moved {text} {before_index}->{after_index}")
                }
            })
            .collect()
    }

    #[test]
    fn identical_input_reports_every_line_unchanged() {
        let out = ops(&["alpha", "beta"], &["alpha", "beta"]);
        assert_eq!(shape(&out), vec!["same alpha 0->0", "same beta 1->1"]);
    }

    #[test]
    fn an_inserted_line_does_not_shift_everything_after_it() {
        // The whole point of FR-CMP-3. Index pairing reports 3 changes here;
        // alignment reports exactly one addition.
        let out = ops(&["a", "b", "c"], &["a", "NEW", "b", "c"]);
        assert_eq!(
            shape(&out),
            vec!["same a 0->0", "added NEW 1", "same b 1->2", "same c 2->3"]
        );
    }

    #[test]
    fn a_deleted_line_is_reported_once() {
        let out = ops(&["a", "GONE", "b"], &["a", "b"]);
        assert_eq!(
            shape(&out),
            vec!["same a 0->0", "removed GONE 1", "same b 2->1"]
        );
    }

    #[test]
    fn a_changed_line_is_a_removal_followed_by_an_addition() {
        // Changed, not moved: the texts differ, so nothing may be paired.
        let out = ops(&["a", "old", "b"], &["a", "new", "b"]);
        assert_eq!(
            shape(&out),
            vec!["same a 0->0", "removed old 1", "added new 1", "same b 2->2"]
        );
    }

    #[test]
    fn comparing_against_an_empty_document_reports_only_removals() {
        let out = ops(&["a", "b"], &[]);
        assert_eq!(shape(&out), vec!["removed a 0", "removed b 1"]);
    }

    #[test]
    fn two_empty_documents_produce_no_operations() {
        assert_eq!(ops(&[], &[]), Vec::new());
    }

    #[test]
    fn oversized_input_falls_back_and_says_so() {
        // GR-7: the alignment table is bounded, and the caller is told the
        // result is no longer reflow-resilient rather than being misled.
        let big: Vec<&str> = vec!["x"; MAX_ALIGNED_LINES + 1];
        let (_, quality) = diff_lines(&big, &["x"]);
        assert_eq!(quality, DiffQuality::PositionalFallback);
    }

    // ---- FR-CMP-2: moved where detectable ----

    #[test]
    fn a_relocated_line_is_one_move_not_a_deletion_and_an_insertion() {
        // A clause moved to the end of the document. Reported as two unrelated
        // changes, a reviewer has to notice the text is identical themselves.
        let out = ops(&["clause", "a", "b"], &["a", "b", "clause"]);
        let moves: Vec<_> = shape(&out)
            .into_iter()
            .filter(|line| line.starts_with("moved"))
            .collect();
        assert_eq!(moves, vec!["moved clause 0->2"], "{:?}", shape(&out));
        assert!(
            !shape(&out).iter().any(|line| line.starts_with("removed") || line.starts_with("added")),
            "a pure move must leave no unpaired halves: {:?}",
            shape(&out)
        );
    }

    #[test]
    fn a_line_that_moved_and_changed_is_not_claimed_as_a_move() {
        // "Moved where detectable" — an edited relocation is not detectable by
        // identity pairing, and reporting it as a move would overstate what was
        // established. [FR-CMP-2, PRIN-6]
        let out = ops(&["clause one", "a"], &["a", "clause two"]);
        assert!(
            !shape(&out).iter().any(|line| line.starts_with("moved")),
            "{:?}",
            shape(&out)
        );
    }

    #[test]
    fn repeated_identical_lines_pair_in_order() {
        // Two removals, one addition of the same text: exactly one move, and
        // the leftover stays a removal rather than being invented away.
        let out = ops(&["dup", "dup", "keep"], &["keep", "dup"]);
        let rendered = shape(&out);
        assert_eq!(
            rendered.iter().filter(|line| line.starts_with("moved")).count(),
            1,
            "{rendered:?}"
        );
        assert_eq!(
            rendered.iter().filter(|line| line.starts_with("removed")).count(),
            1,
            "{rendered:?}"
        );
    }

    #[test]
    fn move_detection_survives_the_positional_fallback() {
        // The fallback is less accurate, not less honest: a move that is still
        // visible there is still reported.
        let mut before: Vec<&str> = vec!["filler"; MAX_ALIGNED_LINES];
        before.insert(0, "moved-line");
        let mut after: Vec<&str> = vec!["filler"; MAX_ALIGNED_LINES];
        after.push("moved-line");
        let (out, quality) = diff_lines(&before, &after);
        assert_eq!(quality, DiffQuality::PositionalFallback);
        assert!(
            out.iter().any(|op| matches!(op, LineDiff::Moved { text, .. } if text == "moved-line")),
            "a relocated line should still be paired under the fallback"
        );
    }
}
