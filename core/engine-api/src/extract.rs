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
    pub fn find_all(&self, query: &str) -> Vec<MatchLocation> {
        if query.is_empty() {
            return Vec::new();
        }
        let query_lower = query.to_lowercase();
        let mut results = Vec::new();

        for line in &self.lines {
            let line_lower = line.text.to_lowercase();
            let mut start = 0;
            while let Some(pos) = line_lower[start..].find(&query_lower) {
                let absolute = start + pos;
                results.push(MatchLocation {
                    line_index: line.index,
                    char_offset: absolute as u32,
                    char_len: query.len() as u32,
                });
                start = absolute + 1;
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
