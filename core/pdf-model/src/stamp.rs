//! Watermark, header/footer, and Bates numbering content streams. [FR-STAMP, M6]
//!
//! Generates PDF content stream operators that overlay text on pages.
//! These streams are appended to page `/Contents` during save, making
//! the stamps visible without modifying the original content.
//!
//! Stamps are non-destructive overlays — the original page content is
//! preserved underneath. [PRIN-2]

use std::io::Write;

/// Position for a stamp on the page.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StampPosition {
    /// Top-left corner with margin.
    TopLeft,
    /// Top-center.
    TopCenter,
    /// Top-right corner with margin.
    TopRight,
    /// Center of the page.
    Center,
    /// Bottom-left corner with margin.
    BottomLeft,
    /// Bottom-center.
    BottomCenter,
    /// Bottom-right corner with margin.
    BottomRight,
}

/// A stamp to apply to pages.
#[derive(Debug, Clone)]
pub struct Stamp {
    /// The text to render (watermark text, Bates number, header/footer).
    pub text: String,
    /// Where on the page to place the stamp.
    pub position: StampPosition,
    /// Font size in points.
    pub font_size: f32,
    /// Font name (PDF font reference).
    pub font_name: String,
    /// Text color [r, g, b] (0.0–1.0).
    pub color: [f32; 3],
    /// Opacity (0.0–1.0). Applied via graphics state.
    pub opacity: f32,
    /// Margin from page edge in points.
    pub margin: f32,
    /// Rotation in degrees (0, 90, 180, 270, or arbitrary).
    pub rotation: f32,
}

/// Resource name the stamp's font is registered under.
///
/// The content stream's `Tf` operator and the page's `/Resources /Font` entry
/// must name the same resource or the reference does not resolve: the stream
/// said `/F1` while the page declared `/FStamp`, so a strict reader had no font
/// to draw with. It is deliberately not `F1` — that is the name documents most
/// often already use, and overwriting a page's own `/F1` would restyle its
/// text. [FR-STAMP, PRIN-1]
pub const STAMP_FONT_RESOURCE: &str = "FStamp";

impl Default for Stamp {
    fn default() -> Self {
        Self {
            text: String::new(),
            position: StampPosition::BottomCenter,
            font_size: 10.0,
            font_name: STAMP_FONT_RESOURCE.into(),
            color: [0.0, 0.0, 0.0],
            opacity: 1.0,
            margin: 36.0,
            rotation: 0.0,
        }
    }
}

/// Generate a Bates number string from an index. [FR-STAMP]
///
/// Format: zero-padded number, e.g., "000001", "000002", ...
pub fn bates_number(start: u32, index: u32, width: usize) -> String {
    format!("{:0width$}", start + index, width = width)
}

/// Resource name for the stamp's transparency state, shared by the content
/// stream and the page's `/Resources` so the two can never disagree.
pub const EXT_GSTATE_NAME: &str = "GSstamp";

/// Whether this stamp needs an `/ExtGState` resource at all.
#[must_use]
pub fn needs_ext_gstate(stamp: &Stamp) -> bool {
    (stamp.opacity - 1.0).abs() > 0.001
}

/// Generate a PDF content stream that renders a stamp on a page. [FR-STAMP]
///
/// The stream is in page coordinates (not local widget coords).
/// Page dimensions are needed to compute position.
pub fn generate_stamp_stream(
    stamp: &Stamp,
    page_width: f32,
    page_height: f32,
) -> Vec<u8> {
    let mut buf = Vec::new();
    let m = stamp.margin;

    // Compute position.
    let (x, y) = match stamp.position {
        StampPosition::TopLeft => (m, page_height - m),
        StampPosition::TopCenter => (page_width / 2.0, page_height - m),
        StampPosition::TopRight => (page_width - m, page_height - m),
        StampPosition::Center => (page_width / 2.0, page_height / 2.0),
        StampPosition::BottomLeft => (m, m),
        StampPosition::BottomCenter => (page_width / 2.0, m),
        StampPosition::BottomRight => (page_width - m, m),
    };

    // Save graphics state.
    write!(&mut buf, "q\n").unwrap();

    // Set opacity if not fully opaque.
    //
    // This used to write `{opacity} gs`. The `gs` operator takes the *name* of
    // an ExtGState in the page's resources, not a number, so a translucent
    // stamp emitted a malformed operator that a reader may reject along with
    // the rest of the stream. The name below is the one
    // `page_patch::inject_content_ref_and_font` writes into `/ExtGState`.
    // [FR-STAMP, PRIN-1, GR-8]
    if needs_ext_gstate(stamp) {
        write!(&mut buf, "/{EXT_GSTATE_NAME} gs\n").unwrap();
    }

    // Set text color.
    write!(&mut buf, "{:.3} {:.3} {:.3} rg\n",
        stamp.color[0], stamp.color[1], stamp.color[2]).unwrap();

    // Begin text object.
    write!(&mut buf, "BT\n").unwrap();

    // Set font.
    write!(&mut buf, "/{} {:.1} Tf\n", stamp.font_name, stamp.font_size).unwrap();

    // Apply rotation if non-zero.
    if stamp.rotation.abs() > 0.01 {
        let rad = stamp.rotation * std::f32::consts::PI / 180.0;
        let cos = rad.cos();
        let sin = rad.sin();
        write!(&mut buf, "{:.6} {:.6} {:.6} {:.6} {:.1} {:.1} Tm\n",
            cos, sin, -sin, cos, x, y).unwrap();
    } else {
        // Position text. For bottom positions, use baseline; for top, use descent.
        let text_y = match stamp.position {
            StampPosition::TopLeft | StampPosition::TopCenter | StampPosition::TopRight => {
                y - stamp.font_size
            }
            _ => y,
        };
        write!(&mut buf, "1 0 0 1 {x:.1} {text_y:.1} Tm\n").unwrap();
    }

    // Render text.
    let escaped = escape_stamp_str(&stamp.text);
    write!(&mut buf, "({escaped}) Tj\n").unwrap();

    // End text object.
    write!(&mut buf, "ET\n").unwrap();

    // Restore graphics state.
    write!(&mut buf, "Q\n").unwrap();

    buf
}

/// Generate Bates-stamped content streams for multiple pages. [FR-STAMP]
///
/// Returns a Vec of (page_index, content_stream) pairs.
pub fn generate_bates_stamps(
    start_number: u32,
    page_count: u32,
    width: usize,
    page_width: f32,
    page_height: f32,
    position: StampPosition,
    font_size: f32,
) -> Vec<(u32, Vec<u8>)> {
    let mut results = Vec::new();
    for i in 0..page_count {
        let text = bates_number(start_number, i, width);
        let stamp = Stamp {
            text,
            position,
            font_size,
            ..Stamp::default()
        };
        let stream = generate_stamp_stream(&stamp, page_width, page_height);
        results.push((i, stream));
    }
    results
}

/// Escape a string for PDF literal string syntax.
fn escape_stamp_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '(' => out.push_str("\\("),
            ')' => out.push_str("\\)"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bates_number_formatting() {
        assert_eq!(bates_number(1, 0, 6), "000001");
        assert_eq!(bates_number(1, 5, 6), "000006");
        assert_eq!(bates_number(100, 0, 4), "0100");
        assert_eq!(bates_number(0, 42, 3), "042");
    }

    #[test]
    fn stamp_stream_contains_text() {
        let stamp = Stamp {
            text: "CONFIDENTIAL".into(),
            ..Stamp::default()
        };
        let stream = generate_stamp_stream(&stamp, 612.0, 792.0);
        let s = String::from_utf8_lossy(&stream);
        assert!(s.contains("CONFIDENTIAL"), "stamp stream should contain text");
        assert!(s.contains("BT"), "should have text object");
        assert!(s.contains("ET"), "should close text object");
        assert!(s.contains("q"), "should save graphics state");
        assert!(s.contains("Q"), "should restore graphics state");
    }

    #[test]
    fn stamp_positions_computed_correctly() {
        let stamp = Stamp {
            text: "X".into(),
            position: StampPosition::TopLeft,
            margin: 36.0,
            ..Stamp::default()
        };
        let stream = generate_stamp_stream(&stamp, 612.0, 792.0);
        let s = String::from_utf8_lossy(&stream);
        // TopLeft: x=36, y=792-36=756, adjusted for font size
        assert!(s.contains("36.0"), "should have x=36");
    }

    #[test]
    fn stamp_opacity_applied() {
        let stamp = Stamp {
            text: "fade".into(),
            opacity: 0.5,
            ..Stamp::default()
        };
        let stream = generate_stamp_stream(&stamp, 612.0, 792.0);
        let s = String::from_utf8_lossy(&stream);
        // `contains("gs")` passed for the malformed `0.500 gs` this used to
        // emit. The operand of `gs` must be a name. [T-10]
        assert!(
            s.contains(&format!("/{EXT_GSTATE_NAME} gs")),
            "opacity must reference a named ExtGState, got: {s}"
        );
        assert!(
            !s.contains("0.500 gs"),
            "the operand of `gs` must be a name, not a number: {s}"
        );
    }

    #[test]
    fn stamp_rotation_applied() {
        let stamp = Stamp {
            text: "rotated".into(),
            rotation: 45.0,
            ..Stamp::default()
        };
        let stream = generate_stamp_stream(&stamp, 612.0, 792.0);
        let s = String::from_utf8_lossy(&stream);
        assert!(s.contains("Tm"), "should use text matrix for rotation");
    }

    #[test]
    fn bates_stamps_produce_per_page() {
        let stamps = generate_bates_stamps(1, 5, 6, 612.0, 792.0,
            StampPosition::BottomCenter, 10.0);
        assert_eq!(stamps.len(), 5);
        assert_eq!(stamps[0].0, 0); // page 0
        assert_eq!(stamps[4].0, 4); // page 4
        // First stamp should contain "000001"
        let s = String::from_utf8_lossy(&stamps[0].1);
        assert!(s.contains("000001"));
    }

    #[test]
    fn stamp_escape_handles_parens() {
        assert_eq!(escape_stamp_str("a(b)"), "a\\(b\\)");
        assert_eq!(escape_stamp_str("a\\b"), "a\\\\b");
    }
}
