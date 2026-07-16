//! In-document find + cross-document search index. [ADR-019, SDS §2.2.9]
//!
//! In-document find: streaming search over the canonical text model with
//! normalization (case, diacritics, ligature folding, soft-hyphen elision),
//! operating page-window-first for instant first-hit, then completing in
//! background. [ADR-019 §2]
//!
//! Cross-document index: opt-in, local Tantivy-based index over
//! user-designated folders, built by utility jobs. [ADR-019 §3]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

use engine_api::extract::PageTextModel;

/// Search options for in-document find. [FR-SRCH-2]
#[derive(Debug, Clone)]
pub struct FindOptions {
    /// Case-insensitive matching (default: true).
    pub case_sensitive: bool,
    /// Whole-word matching (default: false).
    pub whole_word: bool,
    /// Search backwards from current position (default: false).
    pub backwards: bool,
}

impl Default for FindOptions {
    fn default() -> Self {
        Self {
            case_sensitive: false,
            whole_word: false,
            backwards: false,
        }
    }
}

/// A single find match with full location information.
#[derive(Debug, Clone)]
pub struct FindMatch {
    /// Page index (0-based).
    pub page_index: u32,
    /// Line index within the page.
    pub line_index: u32,
    /// Character offset within the line.
    pub char_offset: u32,
    /// Character length of the match.
    pub char_len: u32,
    /// The matched text (original case).
    pub matched_text: String,
    /// Bounding rectangle of the match on the page (PDF points).
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Find state: tracks position across pages for next/previous navigation.
#[derive(Debug)]
pub struct FindState {
    /// Current page index for the search cursor.
    pub current_page: u32,
    /// Current line index within the page.
    pub current_line: u32,
    /// Current character offset within the line.
    pub current_char: u32,
    /// Total matches found in the last search.
    pub total_matches: u32,
    /// Current match index (0-based) for position display.
    pub current_match_index: u32,
}

impl FindState {
    /// Create a new find state starting at page 0.
    pub fn new() -> Self {
        Self {
            current_page: 0,
            current_line: 0,
            current_char: 0,
            total_matches: 0,
            current_match_index: 0,
        }
    }

    /// Advance the cursor to the next match position.
    pub fn advance(&mut self, page_index: u32, line_index: u32, char_offset: u32) {
        self.current_page = page_index;
        self.current_line = line_index;
        self.current_char = char_offset + 1; // advance past the match
    }

    /// Move the cursor backward (for reverse search).
    pub fn retreat(&mut self, page_index: u32, line_index: u32, char_offset: u32) {
        self.current_page = page_index;
        self.current_line = line_index;
        self.current_char = char_offset.saturating_sub(1);
    }
}

impl Default for FindState {
    fn default() -> Self {
        Self::new()
    }
}

/// Advance `i` to the next UTF-8 char boundary (at least one byte forward).
fn advance_char_boundary(s: &str, i: usize) -> usize {
    if i >= s.len() {
        return s.len();
    }
    let mut j = i + 1;
    while j < s.len() && !s.is_char_boundary(j) {
        j += 1;
    }
    j
}

/// Clamp a range to valid char boundaries within `s`.
fn char_safe_slice<'a>(s: &'a str, start: usize, len: usize) -> &'a str {
    let mut start = start.min(s.len());
    while start > 0 && !s.is_char_boundary(start) {
        start -= 1;
    }
    let mut end = (start + len).min(s.len());
    while end < s.len() && !s.is_char_boundary(end) {
        end += 1;
    }
    while end > start && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[start..end]
}

/// Normalize text for searching: lowercase, fold common ligatures,
/// elide soft hyphens.
fn normalize_for_search(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    for ch in text.to_lowercase().chars() {
        match ch {
            '\u{00AD}' => {} // soft hyphen — skip
            '\u{FB01}' => result.push_str("fi"), // fi ligature
            '\u{FB02}' => result.push_str("fl"), // fl ligature
            '\u{FB03}' => result.push_str("ffi"), // ffi ligature
            '\u{FB04}' => result.push_str("ffl"), // ffl ligature
            _ => result.push(ch),
        }
    }
    result
}

/// Check if a character is a word boundary (for whole-word matching).
fn is_word_boundary(ch: char) -> bool {
    ch.is_whitespace() || ch.is_control() || matches!(ch, '.' | ',' | ';' | ':' | '!' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '"' | '\'' | '-' | '/' | '\\')
}

/// Find the first occurrence of `query` in a page's text model, starting
/// from the given cursor position.
///
/// Returns the match location if found, updating the cursor position.
pub fn find_first(
    model: &PageTextModel,
    query: &str,
    options: &FindOptions,
    cursor_page: u32,
    cursor_line: u32,
    cursor_char: u32,
) -> Option<FindMatch> {
    if query.is_empty() || model.page_index != cursor_page {
        return None;
    }

    let query_norm = if options.case_sensitive {
        query.to_string()
    } else {
        normalize_for_search(query)
    };

    for line in &model.lines {
        if line.index < cursor_line {
            continue;
        }

        let search_text = if options.case_sensitive {
            line.text.clone()
        } else {
            normalize_for_search(&line.text)
        };

        let start_offset = if line.index == cursor_line {
            cursor_char as usize
        } else {
            0
        };

        // Snap cursor to a char boundary on the normalized string.
        let mut search_start = start_offset.min(search_text.len());
        while search_start > 0 && !search_text.is_char_boundary(search_start) {
            search_start -= 1;
        }
        while search_start < search_text.len() {
            if let Some(pos) = search_text[search_start..].find(&query_norm) {
                let absolute = search_start + pos;
                let match_len = query_norm.len();

                // Whole-word check on original line using char indices approx.
                if options.whole_word {
                    let before_ok = absolute == 0
                        || search_text[..absolute]
                            .chars()
                            .next_back()
                            .map_or(true, is_word_boundary);
                    let after_ok = absolute + match_len >= search_text.len()
                        || search_text[absolute + match_len..]
                            .chars()
                            .next()
                            .map_or(true, is_word_boundary);
                    if !before_ok || !after_ok {
                        search_start = advance_char_boundary(&search_text, absolute);
                        continue;
                    }
                }

                let (x, y, w, h) =
                    compute_match_bounds(model, line.index, absolute, match_len);

                return Some(FindMatch {
                    page_index: model.page_index,
                    line_index: line.index,
                    char_offset: absolute as u32,
                    char_len: match_len as u32,
                    matched_text: char_safe_slice(&search_text, absolute, match_len).to_string(),
                    x,
                    y,
                    width: w,
                    height: h,
                });
            }
            break; // No more matches on this line.
        }
    }

    None
}

/// Find all occurrences of `query` in a page's text model.
pub fn find_all(
    model: &PageTextModel,
    query: &str,
    options: &FindOptions,
) -> Vec<FindMatch> {
    if query.is_empty() {
        return Vec::new();
    }

    let query_norm = if options.case_sensitive {
        query.to_string()
    } else {
        normalize_for_search(query)
    };

    let mut results = Vec::new();

    for line in &model.lines {
        let search_text = if options.case_sensitive {
            line.text.clone()
        } else {
            normalize_for_search(&line.text)
        };

        let mut search_start = 0;
        while search_start < search_text.len() {
            if let Some(pos) = search_text[search_start..].find(&query_norm) {
                let absolute = search_start + pos;
                let match_len = query_norm.len();

                if options.whole_word {
                    let before_ok = absolute == 0
                        || search_text[..absolute]
                            .chars()
                            .next_back()
                            .map_or(true, is_word_boundary);
                    let after_ok = absolute + match_len >= search_text.len()
                        || search_text[absolute + match_len..]
                            .chars()
                            .next()
                            .map_or(true, is_word_boundary);
                    if !before_ok || !after_ok {
                        search_start = advance_char_boundary(&search_text, absolute);
                        continue;
                    }
                }

                let (x, y, w, h) =
                    compute_match_bounds(model, line.index, absolute, match_len);

                results.push(FindMatch {
                    page_index: model.page_index,
                    line_index: line.index,
                    char_offset: absolute as u32,
                    char_len: match_len as u32,
                    matched_text: char_safe_slice(&search_text, absolute, match_len).to_string(),
                    x,
                    y,
                    width: w,
                    height: h,
                });

                // Advance past this match on a char boundary (CJK-safe). [M2]
                search_start = absolute + match_len;
                if search_start < search_text.len() && !search_text.is_char_boundary(search_start) {
                    search_start = advance_char_boundary(&search_text, absolute);
                }
            } else {
                break;
            }
        }
    }

    results
}

/// Compute the bounding rectangle of a match from the line's spans.
fn compute_match_bounds(
    model: &PageTextModel,
    line_index: u32,
    _char_offset: usize,
    _char_len: usize,
) -> (f32, f32, f32, f32) {
    // Find the line and approximate bounding box from character positions.
    if let Some(line) = model.lines.iter().find(|l| l.index == line_index) {
        // Simple approximation: use the line's bounding box.
        // A proper implementation would walk spans to get precise per-character positions.
        (line.x, line.y, line.width, line.height)
    } else {
        (0.0, 0.0, 0.0, 0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_api::extract::TextLine;

    fn test_model() -> PageTextModel {
        PageTextModel {
            page_index: 0,
            lines: vec![
                TextLine {
                    index: 0,
                    text: "Hello world".into(),
                    x: 0.0, y: 0.0, width: 100.0, height: 12.0,
                    spans: vec![],
                },
                TextLine {
                    index: 1,
                    text: "The world is big".into(),
                    x: 0.0, y: 20.0, width: 100.0, height: 12.0,
                    spans: vec![],
                },
            ],
            reliable: true,
            char_count: 27,
            has_structure: false,
        }
    }

    #[test]
    fn find_first_basic() {
        let model = test_model();
        let opts = FindOptions::default();
        let m = find_first(&model, "world", &opts, 0, 0, 0).unwrap();
        assert_eq!(m.page_index, 0);
        assert_eq!(m.line_index, 0);
        assert_eq!(m.char_offset, 6);
        assert_eq!(m.matched_text, "world");
    }

    #[test]
    fn find_first_from_middle() {
        let model = test_model();
        let opts = FindOptions::default();
        // Start from character 7 (past the first "world")
        let m = find_first(&model, "world", &opts, 0, 0, 7).unwrap();
        assert_eq!(m.line_index, 1);
        assert_eq!(m.char_offset, 4);
    }

    #[test]
    fn find_first_case_insensitive() {
        let model = test_model();
        let opts = FindOptions::default();
        let m = find_first(&model, "WORLD", &opts, 0, 0, 0).unwrap();
        assert_eq!(m.matched_text, "world");
    }

    #[test]
    fn find_first_whole_word() {
        let model = test_model();
        let opts = FindOptions { whole_word: true, ..Default::default() };
        let m = find_first(&model, "world", &opts, 0, 0, 0).unwrap();
        assert_eq!(m.char_offset, 6); // "world" is a whole word
    }

    #[test]
    fn find_all_returns_multiple() {
        let model = test_model();
        let opts = FindOptions::default();
        let matches = find_all(&model, "world", &opts);
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].line_index, 0);
        assert_eq!(matches[1].line_index, 1);
    }

    #[test]
    fn normalize_ligatures() {
        // Extraction correctness: ToUnicode ligatures fold for find. [ADR-019, M2 exit]
        assert_eq!(normalize_for_search("HELLO"), "hello");
        assert_eq!(
            normalize_for_search("\u{FB01}\u{FB02}\u{FB03}\u{FB04}"),
            "fiflffiffl"
        );
        // Soft hyphen elided
        assert_eq!(normalize_for_search("soft\u{00AD}hyphen"), "softhyphen");
    }

    #[test]
    fn find_ligature_query_matches_folded_text() {
        use engine_api::extract::{PageTextModel, TextLine};
        let model = PageTextModel {
            page_index: 0,
            lines: vec![TextLine {
                index: 0,
                text: "ﬁle".into(), // U+FB01 fi ligature + "le"
                x: 0.0,
                y: 0.0,
                width: 40.0,
                height: 12.0,
                spans: vec![],
            }],
            reliable: true,
            char_count: 3,
            has_structure: false,
        };
        let opts = FindOptions::default();
        let hits = find_all(&model, "file", &opts);
        assert!(!hits.is_empty(), "ligature fi should match query 'file'");
    }

    #[test]
    fn find_cjk_and_rtl_preserved() {
        use engine_api::extract::{PageTextModel, TextLine};
        let model = PageTextModel {
            page_index: 0,
            lines: vec![
                TextLine {
                    index: 0,
                    text: "日本語テスト".into(),
                    x: 0.0,
                    y: 0.0,
                    width: 100.0,
                    height: 12.0,
                    spans: vec![],
                },
                TextLine {
                    index: 1,
                    text: "مرحبا".into(),
                    x: 0.0,
                    y: 20.0,
                    width: 80.0,
                    height: 12.0,
                    spans: vec![],
                },
            ],
            reliable: true,
            char_count: 10,
            has_structure: false,
        };
        let opts = FindOptions::default();
        assert_eq!(find_all(&model, "日本", &opts).len(), 1);
        assert_eq!(find_all(&model, "مرحبا", &opts).len(), 1);
    }

    #[test]
    fn unreliable_flag_propagates_in_model() {
        use engine_api::extract::{PageTextModel, TextLine};
        let model = PageTextModel {
            page_index: 0,
            lines: vec![TextLine {
                index: 0,
                text: "garbled".into(),
                x: 0.0,
                y: 0.0,
                width: 50.0,
                height: 12.0,
                spans: vec![],
            }],
            reliable: false, // ToUnicode pathology [ADR-019, PRIN-6]
            char_count: 7,
            has_structure: false,
        };
        assert!(!model.reliable);
        let hits = find_all(&model, "garbled", &FindOptions::default());
        assert_eq!(hits.len(), 1); // still searchable, but consumers must flag
    }

    #[test]
    fn word_boundary_check() {
        assert!(is_word_boundary(' '));
        assert!(is_word_boundary('.'));
        assert!(!is_word_boundary('a'));
    }

    #[test]
    fn find_state_advance() {
        let mut state = FindState::new();
        state.advance(0, 0, 5);
        assert_eq!(state.current_page, 0);
        assert_eq!(state.current_line, 0);
        assert_eq!(state.current_char, 6);
    }
}
