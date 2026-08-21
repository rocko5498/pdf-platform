//! Reflow-resilient line comparison for document diff. [FR-CMP-1, FR-CMP-3]
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineDiff {
    /// Present in both sides, unchanged.
    Same(String),
    /// Present only in the earlier document.
    Removed(String),
    /// Present only in the later document.
    Added(String),
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
        return (positional(before, after), DiffQuality::PositionalFallback);
    }
    (aligned(before, after), DiffQuality::Aligned)
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
            ops.push(LineDiff::Same(before[i].to_owned()));
            i += 1;
            j += 1;
        } else if lengths[(i + 1) * stride + j] >= lengths[i * stride + j + 1] {
            ops.push(LineDiff::Removed(before[i].to_owned()));
            i += 1;
        } else {
            ops.push(LineDiff::Added(after[j].to_owned()));
            j += 1;
        }
    }
    ops.extend(before[i..].iter().map(|l| LineDiff::Removed((*l).to_owned())));
    ops.extend(after[j..].iter().map(|l| LineDiff::Added((*l).to_owned())));
    ops
}

fn positional(before: &[&str], after: &[&str]) -> Vec<LineDiff> {
    let mut ops = Vec::new();
    for index in 0..before.len().max(after.len()) {
        match (before.get(index), after.get(index)) {
            (Some(b), Some(a)) if b == a => ops.push(LineDiff::Same((*b).to_owned())),
            (Some(b), Some(a)) => {
                ops.push(LineDiff::Removed((*b).to_owned()));
                ops.push(LineDiff::Added((*a).to_owned()));
            }
            (Some(b), None) => ops.push(LineDiff::Removed((*b).to_owned())),
            (None, Some(a)) => ops.push(LineDiff::Added((*a).to_owned())),
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

    #[test]
    fn identical_input_reports_every_line_unchanged() {
        let out = ops(&["alpha", "beta"], &["alpha", "beta"]);
        assert_eq!(
            out,
            vec![
                LineDiff::Same("alpha".into()),
                LineDiff::Same("beta".into())
            ]
        );
    }

    #[test]
    fn an_inserted_line_does_not_shift_everything_after_it() {
        // The whole point of FR-CMP-3. Index pairing reports 3 changes here;
        // alignment reports exactly one addition.
        let out = ops(&["a", "b", "c"], &["a", "NEW", "b", "c"]);
        assert_eq!(
            out,
            vec![
                LineDiff::Same("a".into()),
                LineDiff::Added("NEW".into()),
                LineDiff::Same("b".into()),
                LineDiff::Same("c".into()),
            ]
        );
    }

    #[test]
    fn a_deleted_line_is_reported_once() {
        let out = ops(&["a", "GONE", "b"], &["a", "b"]);
        assert_eq!(
            out,
            vec![
                LineDiff::Same("a".into()),
                LineDiff::Removed("GONE".into()),
                LineDiff::Same("b".into()),
            ]
        );
    }

    #[test]
    fn a_changed_line_is_a_removal_followed_by_an_addition() {
        let out = ops(&["a", "old", "b"], &["a", "new", "b"]);
        assert!(out.contains(&LineDiff::Removed("old".into())), "{out:?}");
        assert!(out.contains(&LineDiff::Added("new".into())), "{out:?}");
        assert_eq!(
            out.iter().filter(|o| matches!(o, LineDiff::Same(_))).count(),
            2
        );
    }

    #[test]
    fn comparing_against_an_empty_document_reports_only_removals() {
        let out = ops(&["a", "b"], &[]);
        assert_eq!(
            out,
            vec![
                LineDiff::Removed("a".into()),
                LineDiff::Removed("b".into())
            ]
        );
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
}
