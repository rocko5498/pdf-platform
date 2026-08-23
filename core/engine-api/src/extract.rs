//! Text-extraction engine trait. [ADR-005, ADR-019, GR-4]
//!
//! Defines the canonical text model that the extraction service consumes.
//! The engine produces `PageTextModel` per page; all consumers (find,
//! selection, copy, accessibility export, indexing) share this single model
//! — the single-extraction invariant. [ADR-019 §1]

/// A single text span on a page: Unicode text + bounding geometry.
#[derive(Debug, Clone)]
pub struct TextSpan {
    /// The Unicode text content of this span.
    pub text: String,
    /// Bounding rectangle in PDF user-space coordinates (points).
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// 0-based index of the line this span belongs to.
    pub line_index: u32,
    /// 0-based index of the word within its line.
    pub word_index: u32,
    /// Whether this span came from a tagged structure tree.
    pub is_structured: bool,
}

/// A line of text on a page, containing one or more spans.
#[derive(Debug, Clone)]
pub struct TextLine {
    /// 0-based line index on this page.
    pub index: u32,
    /// The concatenated text of all spans in this line.
    pub text: String,
    /// Bounding box of the entire line.
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// Spans within this line, in reading order.
    pub spans: Vec<TextSpan>,
}

/// Canonical per-page text model. [ADR-019]
///
/// Produced by the engine's Extract trait, cached revision-keyed,
/// and consumed by find, selection, copy, accessibility export,
/// and cross-document indexing — the single-extraction invariant.
#[derive(Debug, Clone)]
pub struct PageTextModel {
    /// 0-based page index.
    pub page_index: u32,
    /// All text lines on this page, in reading order.
    pub lines: Vec<TextLine>,
    /// Whether this page's text layer is reliable.
    ///
    /// False when the ToUnicode map is missing, incomplete, or produces
    /// obviously wrong characters. Consumers MUST flag unreliable pages
    /// rather than silently searching incorrect text. [ADR-019 §4, PRIN-6]
    pub reliable: bool,
    /// Total character count across all lines.
    pub char_count: u32,
    /// Whether the page has a tagged structure tree (structured reading order).
    pub has_structure: bool,
}

impl PageTextModel {
    /// Full text of the page, lines joined by newlines.
    pub fn full_text(&self) -> String {
        self.lines.iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Find all occurrences of a query (case-insensitive) and return their
    /// line and character offsets.
    ///
    /// This is literal substring matching. It applies **none** of the
    /// normalization FR-SRCH-1 requires — ligature folding, soft-hyphen
    /// elision, diacritics — so product search goes through
    /// `search::find_all` instead. This remains for callers that genuinely
    /// want the raw text.
    ///
    /// Offsets are in **characters**, as the field names say. They were byte
    /// offsets: on any line containing a multi-byte character the values were
    /// wrong, and `start + 1` advanced by one byte, so the next slice landed
    /// inside a codepoint and panicked — reachable from any document with
    /// non-ASCII text and a search box. [FR-SRCH-1, FR-SRCH-2, PRIN-1]
    pub fn find_all(&self, query: &str) -> Vec<MatchLocation> {
        if query.is_empty() {
            return Vec::new();
        }
        let query_lower: Vec<char> = query.to_lowercase().chars().collect();
        let mut results = Vec::new();

        for line in &self.lines {
            let haystack: Vec<char> = line.text.to_lowercase().chars().collect();
            if query_lower.len() > haystack.len() {
                continue;
            }
            for start in 0..=(haystack.len() - query_lower.len()) {
                if haystack[start..start + query_lower.len()] == query_lower[..] {
                    results.push(MatchLocation {
                        line_index: line.index,
                        char_offset: start as u32,
                        char_len: query_lower.len() as u32,
                    });
                }
            }
        }

        results
    }
}

/// Location of a text match within a page.
#[derive(Debug, Clone)]
pub struct MatchLocation {
    /// Line index containing the match.
    pub line_index: u32,
    /// Character offset within the line.
    pub char_offset: u32,
    /// Character length of the match.
    pub char_len: u32,
}

/// Error from text extraction.
#[derive(Debug)]
pub enum ExtractError {
    /// Page index out of range.
    PageOutOfRange { requested: u32, page_count: u32 },
    /// Engine-specific extraction failure.
    Engine(String),
    /// Extraction produced no text for this page (image-only page).
    EmptyPage,
}

impl std::fmt::Display for ExtractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PageOutOfRange { requested, page_count } =>
                write!(f, "page {requested} out of range (0..{page_count})"),
            Self::Engine(msg) => write!(f, "extraction error: {msg}"),
            Self::EmptyPage => write!(f, "no text on this page"),
        }
    }
}

impl std::error::Error for ExtractError {}

/// Text-extraction capability. [ADR-005, GR-4]
///
/// The engine produces a `PageTextModel` per page. The extraction service
/// caches these revision-keyed and serves all consumers from the cache.
pub trait Extract: Send + Sync {
    /// Extract the canonical text model for a single page.
    fn extract_page(&self, page_index: u32) -> Result<PageTextModel, ExtractError>;

    /// Page count (for bounds checking).
    fn page_count(&self) -> u32;
}

#[cfg(test)]
mod tests {

    #[test]
    fn matching_multibyte_text_does_not_panic() {
        // `find_all` sliced the line by byte index and advanced `start + 1`
        // byte at a time, so the next slice landed inside a codepoint and
        // panicked — reachable from any document with non-ASCII text and a
        // search box. [PRIN-1, FR-SRCH-1]
        let model = PageTextModel {
            page_index: 0,
            lines: vec![TextLine {
                index: 0,
                text: "\u{E000}\u{E001}\u{E002}".to_owned(),
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
                spans: Vec::new(),
            }],
            reliable: false,
            char_count: 3,
            has_structure: false,
        };
        let hits = model.find_all("\u{E000}");
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!(hits[0].char_offset, 0);
        assert_eq!(hits[0].char_len, 1);
    }

    #[test]
    fn offsets_are_characters_not_bytes() {
        // The fields are named char_offset and char_len. They held byte values,
        // so every consumer that mapped a match back onto the page — selection
        // rectangles, highlight geometry — was wrong on any non-ASCII line.
        let model = PageTextModel {
            page_index: 0,
            lines: vec![TextLine {
                index: 0,
                text: "\u{6587}\u{5B57}needle".to_owned(),
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
                spans: Vec::new(),
            }],
            reliable: true,
            char_count: 8,
            has_structure: false,
        };
        let hits = model.find_all("needle");
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!(
            hits[0].char_offset, 2,
            "two CJK characters precede the match, not six bytes"
        );
        assert_eq!(hits[0].char_len, 6);
    }

    use super::*;

    #[test]
    fn page_text_model_find_all() {
        let model = PageTextModel {
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
        };

        let matches = model.find_all("world");
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].line_index, 0);
        assert_eq!(matches[0].char_offset, 6);
        assert_eq!(matches[1].line_index, 1);
        assert_eq!(matches[1].char_offset, 4);
    }

    #[test]
    fn find_all_case_insensitive() {
        let model = PageTextModel {
            page_index: 0,
            lines: vec![TextLine {
                index: 0,
                text: "Hello WORLD".into(),
                x: 0.0, y: 0.0, width: 100.0, height: 12.0,
                spans: vec![],
            }],
            reliable: true,
            char_count: 11,
            has_structure: false,
        };

        let matches = model.find_all("world");
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn find_all_empty_query() {
        let model = PageTextModel {
            page_index: 0,
            lines: vec![],
            reliable: true,
            char_count: 0,
            has_structure: false,
        };
        assert!(model.find_all("").is_empty());
    }

    #[test]
    fn full_text_joins_lines() {
        let model = PageTextModel {
            page_index: 0,
            lines: vec![
                TextLine { index: 0, text: "line one".into(), x: 0.0, y: 0.0, width: 100.0, height: 12.0, spans: vec![] },
                TextLine { index: 1, text: "line two".into(), x: 0.0, y: 20.0, width: 100.0, height: 12.0, spans: vec![] },
            ],
            reliable: true,
            char_count: 17,
            has_structure: false,
        };
        assert_eq!(model.full_text(), "line one\nline two");
    }
}
