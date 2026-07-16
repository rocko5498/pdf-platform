//! Annotation model: types, properties, and appearance streams. [FR-ANNOT, SDS §2.2.9]
//!
//! Annotations are the most common professional workflow after reading.
//! Every annotation written by the Platform MUST include a complete,
//! portable visual appearance (FR-ANNOT-2). The model supports:
//! - Text markup (highlight, underline, strikeout, squiggly)
//! - Sticky notes
//! - Free text
//! - Ink/drawing
//! - Shapes (line, arrow, rectangle, ellipse, polygon, polyline)
//! - Stamps
//! - Callouts
//!
//! All mutations go through Commands (FR-ANNOT-4, ADR-013).

use std::collections::HashMap;

/// Annotation type. [FR-ANNOT-1]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AnnotationType {
    /// Text markup: highlight, underline, strikeout, squiggly.
    TextMarkup(TextMarkupKind),
    /// Sticky note (popup comment).
    StickyNote,
    /// Free text annotation (text box on the page).
    FreeText,
    /// Ink/freehand drawing.
    Ink,
    /// Line annotation.
    Line,
    /// Arrow annotation.
    Arrow,
    /// Rectangle annotation.
    Rectangle,
    /// Ellipse/circle annotation.
    Ellipse,
    /// Polygon annotation.
    Polygon,
    /// Polyline annotation.
    Polyline,
    /// Stamp annotation.
    Stamp,
    /// Callout annotation (text box with leader line).
    Callout,
    /// Redaction annotation (content removal).
    Redaction,
}

/// Text markup sub-type. [FR-ANNOT-1]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextMarkupKind {
    /// Yellow highlight (default).
    Highlight,
    /// Underline.
    Underline,
    /// Strikeout.
    Strikeout,
    /// Squiggly underline.
    Squiggly,
}

impl TextMarkupKind {
    /// Default color for this markup type.
    pub fn default_color(&self) -> Color {
        match self {
            Self::Highlight => Color { r: 1.0, g: 1.0, b: 0.0, a: 0.5 }, // Yellow
            Self::Underline => Color { r: 0.0, g: 1.0, b: 0.0, a: 1.0 }, // Green
            Self::Strikeout => Color { r: 1.0, g: 0.0, b: 0.0, a: 1.0 }, // Red
            Self::Squiggly => Color { r: 1.0, g: 0.0, b: 1.0, a: 1.0 }, // Magenta
        }
    }
}

/// RGBA color. [FR-ANNOT-5]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    /// Create a new color.
    pub fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// Opaque black.
    pub fn black() -> Self {
        Self { r: 0.0, g: 0.0, b: 0.0, a: 1.0 }
    }

    /// Convert to PDF color array [R G B].
    pub fn to_pdf_array(&self) -> [f32; 3] {
        [self.r, self.g, self.b]
    }
}

impl Default for Color {
    fn default() -> Self {
        Self::black()
    }
}

/// Rectangle in PDF user-space coordinates (points). [FR-ANNOT-4]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self { x, y, width, height }
    }

    /// Right edge (x + width).
    pub fn right(&self) -> f32 {
        self.x + self.width
    }

    /// Bottom edge (y + height in PDF coordinates where y goes up).
    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }
}

/// Line style for shape annotations. [FR-ANNOT-5]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineStyle {
    /// Solid line (default).
    Solid,
    /// Dashed line.
    Dashed,
    /// Dotted line.
    Dotted,
}

/// Border/effect for annotations.
#[derive(Debug, Clone)]
pub struct BorderStyle {
    /// Width in points.
    pub width: f32,
    /// Line style.
    pub style: LineStyle,
    /// Dash pattern (for dashed style): [dash, gap, ...].
    pub dash_pattern: Vec<f32>,
}

impl Default for BorderStyle {
    fn default() -> Self {
        Self {
            width: 1.0,
            style: LineStyle::Solid,
            dash_pattern: Vec::new(),
        }
    }
}

/// A point in PDF user-space coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// Annotation properties. [FR-ANNOT-5]
#[derive(Debug, Clone)]
pub struct AnnotationProperties {
    /// Annotation color.
    pub color: Color,
    /// Opacity (0.0 = transparent, 1.0 = opaque).
    pub opacity: f32,
    /// Border style.
    pub border: BorderStyle,
    /// Author name.
    pub author: String,
    /// Creation timestamp (Unix epoch seconds).
    pub creation_time: u64,
    /// Modification timestamp (Unix epoch seconds).
    pub mod_time: u64,
    /// Subject line.
    pub subject: String,
    /// Free-text content (for notes, free text, callouts).
    pub contents: String,
    /// Intent (e.g., "FreeText", "Stamp").
    pub intent: Option<String>,
    /// Flags (PDF annotation flags bitfield).
    pub flags: u32,
    /// Custom properties.
    pub custom: HashMap<String, String>,
}

impl Default for AnnotationProperties {
    fn default() -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self {
            color: Color::black(),
            opacity: 1.0,
            border: BorderStyle::default(),
            author: String::new(),
            creation_time: now,
            mod_time: now,
            subject: String::new(),
            contents: String::new(),
            intent: None,
            flags: 0,
            custom: HashMap::new(),
        }
    }
}

/// QuadPoints for text-markup annotations: the four corners of the
/// text region being marked up. [FR-ANNOT-3]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QuadPoints {
    /// Top-left corner.
    pub top_left: Point,
    /// Top-right corner.
    pub top_right: Point,
    /// Bottom-left corner.
    pub bottom_left: Point,
    /// Bottom-right corner.
    pub bottom_right: Point,
}

impl QuadPoints {
    /// Create QuadPoints from a rectangle (horizontal text).
    pub fn from_rect(rect: &Rect) -> Self {
        Self {
            top_left: Point::new(rect.x, rect.y + rect.height),
            top_right: Point::new(rect.x + rect.width, rect.y + rect.height),
            bottom_left: Point::new(rect.x, rect.y),
            bottom_right: Point::new(rect.x + rect.width, rect.y),
        }
    }
}

/// An annotation on a page. [FR-ANNOT]
///
/// Each annotation knows its type, location, properties, and optionally
/// its appearance stream bytes. The appearance stream is the self-contained
/// visual representation that other readers use to render the annotation. [FR-ANNOT-2]
#[derive(Debug, Clone)]
pub struct Annotation {
    /// Unique identifier for this annotation (coordinator-assigned).
    pub id: u64,
    /// 0-based page index.
    pub page_index: u32,
    /// Annotation type.
    pub annotation_type: AnnotationType,
    /// Bounding rectangle in PDF user-space coordinates.
    pub rect: Rect,
    /// Properties (color, opacity, author, etc.).
    pub properties: AnnotationProperties,
    /// Appearance stream bytes (PDF content stream). [FR-ANNOT-2]
    ///
    /// If present, this is the canonical visual representation.
    /// If absent, the appearance must be synthesized from properties + geometry.
    pub appearance: Option<Vec<u8>>,
    /// QuadPoints for text-markup annotations. [FR-ANNOT-3]
    pub quad_points: Option<QuadPoints>,
    /// Ink path points for ink annotations.
    pub ink_points: Vec<Vec<Point>>,
    /// Shape points for line/arrow/polygon/polyline annotations.
    pub shape_points: Vec<Point>,
    /// Review status (for comment threading).
    pub review_status: ReviewStatus,
    /// Parent annotation ID (for threaded replies). 0 = top-level.
    pub parent_id: u64,
    /// Reply thread: list of child annotation IDs.
    pub replies: Vec<u64>,
}

/// Review status for annotations/comments. [FR-REV]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReviewStatus {
    /// No review status set.
    None,
    /// Marked as accepted.
    Accepted,
    /// Marked as rejected.
    Rejected,
    /// Marked as completed.
    Completed,
    /// Cancelled.
    Cancelled,
}

impl Annotation {
    /// Create a new annotation.
    pub fn new(
        id: u64,
        page_index: u32,
        annotation_type: AnnotationType,
        rect: Rect,
    ) -> Self {
        Self {
            id,
            page_index,
            annotation_type,
            rect,
            properties: AnnotationProperties::default(),
            appearance: None,
            quad_points: None,
            ink_points: Vec::new(),
            shape_points: Vec::new(),
            review_status: ReviewStatus::None,
            parent_id: 0,
            replies: Vec::new(),
        }
    }

    /// Set the author.
    pub fn with_author(mut self, author: impl Into<String>) -> Self {
        self.properties.author = author.into();
        self
    }

    /// Set the color.
    pub fn with_color(mut self, color: Color) -> Self {
        self.properties.color = color;
        self
    }

    /// Set the contents (text).
    pub fn with_contents(mut self, contents: impl Into<String>) -> Self {
        self.properties.contents = contents.into();
        self
    }

    /// Whether this annotation has an embedded appearance stream.
    pub fn has_appearance(&self) -> bool {
        self.appearance.is_some()
    }

    /// Ensure a complete appearance stream is present. [FR-ANNOT-2]
    ///
    /// Generates one if missing so the Platform never writes appearance-less
    /// annotations (PRIN-7, SDS §2.9).
    pub fn ensure_appearance(&mut self) {
        if self.appearance.is_none() {
            self.appearance = Some(crate::appearance::generate_appearance(self));
        }
    }

    /// The annotation's PDF type string.
    pub fn pdf_type_str(&self) -> &'static str {
        match self.annotation_type {
            AnnotationType::TextMarkup(_) => "Highlight", // Sub-type determined by /Subtype
            AnnotationType::StickyNote => "Text",
            AnnotationType::FreeText => "FreeText",
            AnnotationType::Ink => "Ink",
            AnnotationType::Line => "Line",
            AnnotationType::Arrow => "Line", // Arrow is a line with ending style
            AnnotationType::Rectangle => "Square",
            AnnotationType::Ellipse => "Circle",
            AnnotationType::Polygon => "Polygon",
            AnnotationType::Polyline => "Polyline",
            AnnotationType::Stamp => "Stamp",
            AnnotationType::Callout => "FreeText", // Callout is a FreeText variant
            AnnotationType::Redaction => "Redact",
        }
    }

    /// The annotation's PDF subtype string for text markup.
    pub fn pdf_subtype_str(&self) -> Option<&'static str> {
        match self.annotation_type {
            AnnotationType::TextMarkup(kind) => Some(match kind {
                TextMarkupKind::Highlight => "Highlight",
                TextMarkupKind::Underline => "Underline",
                TextMarkupKind::Strikeout => "StrikeOut",
                TextMarkupKind::Squiggly => "Squiggly",
            }),
            _ => None,
        }
    }
}

/// A page containing annotations.
#[derive(Debug, Clone)]
pub struct AnnotationPage {
    /// 0-based page index.
    pub page_index: u32,
    /// Annotations on this page, ordered by creation time.
    pub annotations: Vec<Annotation>,
}

impl AnnotationPage {
    pub fn new(page_index: u32) -> Self {
        Self {
            page_index,
            annotations: Vec::new(),
        }
    }

    /// Add an annotation to this page.
    pub fn add(&mut self, annotation: Annotation) {
        self.annotations.push(annotation);
    }

    /// Remove an annotation by ID.
    pub fn remove(&mut self, id: u64) -> Option<Annotation> {
        self.annotations.iter().position(|a| a.id == id)
            .map(|i| self.annotations.remove(i))
    }

    /// Find an annotation by ID.
    pub fn get(&self, id: u64) -> Option<&Annotation> {
        self.annotations.iter().find(|a| a.id == id)
    }

    /// Find an annotation by ID (mutable).
    pub fn get_mut(&mut self, id: u64) -> Option<&mut Annotation> {
        self.annotations.iter_mut().find(|a| a.id == id)
    }

    /// Number of annotations.
    pub fn len(&self) -> usize {
        self.annotations.len()
    }

    /// Whether the page has no annotations.
    pub fn is_empty(&self) -> bool {
        self.annotations.is_empty()
    }
}

/// Document-level annotation store.
#[derive(Debug, Clone)]
pub struct AnnotationStore {
    /// Pages with annotations.
    pages: HashMap<u32, AnnotationPage>,
    /// Next annotation ID.
    next_id: u64,
}

impl AnnotationStore {
    pub fn new() -> Self {
        Self {
            pages: HashMap::new(),
            next_id: 1,
        }
    }

    /// Allocate a new annotation ID.
    pub fn next_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Get or create the annotation page for a given page index.
    pub fn page_mut(&mut self, page_index: u32) -> &mut AnnotationPage {
        self.pages.entry(page_index).or_insert_with(|| AnnotationPage::new(page_index))
    }

    /// Get the annotation page for a given page index.
    pub fn page(&self, page_index: u32) -> Option<&AnnotationPage> {
        self.pages.get(&page_index)
    }

    /// Get all annotations across all pages.
    pub fn all_annotations(&self) -> Vec<&Annotation> {
        self.pages.values()
            .flat_map(|p| p.annotations.iter())
            .collect()
    }

    /// Total annotation count.
    pub fn total_count(&self) -> usize {
        self.pages.values().map(|p| p.annotations.len()).sum()
    }

    /// Find an annotation by ID across all pages.
    pub fn find(&self, id: u64) -> Option<&Annotation> {
        self.pages.values()
            .flat_map(|p| p.annotations.iter())
            .find(|a| a.id == id)
    }

    /// Find an annotation by ID (mutable).
    pub fn find_mut(&mut self, id: u64) -> Option<&mut Annotation> {
        self.pages.values_mut()
            .flat_map(|p| p.annotations.iter_mut())
            .find(|a| a.id == id)
    }
}

impl Default for AnnotationStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn annotation_create_and_properties() {
        let ann = Annotation::new(1, 0, AnnotationType::StickyNote, Rect::new(100.0, 200.0, 20.0, 20.0))
            .with_author("Test User")
            .with_color(Color::new(1.0, 0.0, 0.0, 1.0))
            .with_contents("This is a note");

        assert_eq!(ann.id, 1);
        assert_eq!(ann.page_index, 0);
        assert_eq!(ann.properties.author, "Test User");
        assert_eq!(ann.properties.contents, "This is a note");
        assert_eq!(ann.pdf_type_str(), "Text");
    }

    #[test]
    fn annotation_store_add_remove() {
        let mut store = AnnotationStore::new();
        let id = store.next_id();
        let ann = Annotation::new(id, 0, AnnotationType::StickyNote, Rect::new(0.0, 0.0, 20.0, 20.0));

        store.page_mut(0).add(ann);
        assert_eq!(store.total_count(), 1);
        assert!(store.find(id).is_some());

        store.page_mut(0).remove(id);
        assert_eq!(store.total_count(), 0);
        assert!(store.find(id).is_none());
    }

    #[test]
    fn text_markup_quad_points() {
        let rect = Rect::new(10.0, 20.0, 100.0, 12.0);
        let qp = QuadPoints::from_rect(&rect);

        assert_eq!(qp.top_left, Point::new(10.0, 32.0));
        assert_eq!(qp.top_right, Point::new(110.0, 32.0));
        assert_eq!(qp.bottom_left, Point::new(10.0, 20.0));
        assert_eq!(qp.bottom_right, Point::new(110.0, 20.0));
    }

    #[test]
    fn text_markup_default_colors() {
        assert_eq!(TextMarkupKind::Highlight.default_color().r, 1.0);
        assert_eq!(TextMarkupKind::Strikeout.default_color().r, 1.0);
        assert_eq!(TextMarkupKind::Underline.default_color().g, 1.0);
    }

    #[test]
    fn annotation_threading() {
        let mut store = AnnotationStore::new();

        let parent_id = store.next_id();
        let parent = Annotation::new(parent_id, 0, AnnotationType::StickyNote, Rect::new(0.0, 0.0, 20.0, 20.0))
            .with_contents("Original comment");
        store.page_mut(0).add(parent);

        let reply_id = store.next_id();
        let mut reply = Annotation::new(reply_id, 0, AnnotationType::StickyNote, Rect::new(0.0, 0.0, 20.0, 20.0))
            .with_contents("Reply to comment");
        reply.parent_id = parent_id;
        store.page_mut(0).add(reply);

        // Link reply to parent.
        if let Some(p) = store.find_mut(parent_id) {
            p.replies.push(reply_id);
        }

        let parent = store.find(parent_id).unwrap();
        assert_eq!(parent.replies.len(), 1);
        assert_eq!(parent.replies[0], reply_id);

        let reply = store.find(reply_id).unwrap();
        assert_eq!(reply.parent_id, parent_id);
    }

    #[test]
    fn review_status() {
        let mut ann = Annotation::new(1, 0, AnnotationType::StickyNote, Rect::new(0.0, 0.0, 20.0, 20.0));
        assert_eq!(ann.review_status, ReviewStatus::None);

        ann.review_status = ReviewStatus::Accepted;
        assert_eq!(ann.review_status, ReviewStatus::Accepted);
    }
}
