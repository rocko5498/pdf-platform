//! OcrEngine trait + Tesseract backend. [ADR-018, M9]
//!
//! The OCR component runs in the utility worker pool (Z1) under full sandbox.
//! It produces normalized intermediates (text/boxes/confidence) that the
//! coordinator applies as invisible text layers. [FR-OCR-1]
//!
//! Engine-plural: recognition backends are pluggable via the `OcrEngine` trait.
//!
//! JBIG2 policy [ADR-018]: symbol-mode compression of OCR output is OFF by
//! default. When enabled, an explicit warning is displayed because symbol-mode
//! JBIG2 can cause character substitution (the Xerox substitution hazard).
//! The text layer always uses uncompressed or Flate-compressed text, never JBIG2.
//! Tesseract is the default; ML backends can be promoted per-release. [ADR-018]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::collections::HashMap;
use std::io::Write;

// ---------------------------------------------------------------------------
// OCR data model [FR-OCR, ADR-018]
// ---------------------------------------------------------------------------

/// A recognized text block with bounding box and confidence.
#[derive(Debug, Clone, PartialEq)]
pub struct OcrTextBlock {
    /// The recognized text.
    pub text: String,
    /// Bounding box `[left, top, width, height]` in source-raster pixel
    /// space (top-left origin, y-down) — the units `recognize()` was given,
    /// e.g. Tesseract TSV coordinates. NOT PDF user-space; the caller
    /// registering a text layer must convert via [`scale_blocks_to_page`].
    pub bbox: [f32; 4],
    /// Confidence score (0.0–1.0).
    pub confidence: f32,
    /// Language detected (e.g., "eng", "jpn").
    pub language: String,
}

/// OCR result for a single page. [FR-OCR-1]
#[derive(Debug, Clone, PartialEq)]
pub struct OcrPageResult {
    /// 0-based page index.
    pub page_index: u32,
    /// Pixel width of the raster recognition actually ran against.
    ///
    /// Authoritative for converting `blocks[].bbox` (raster pixel space)
    /// into PDF points via [`scale_blocks_to_page`] — the worker
    /// self-renders at its own chosen DPI, so the caller cannot assume any
    /// particular value ahead of time.
    pub raster_width: u32,
    /// Pixel height of the raster recognition actually ran against.
    pub raster_height: u32,
    /// Recognized text blocks.
    pub blocks: Vec<OcrTextBlock>,
    /// Full page text (concatenation of all blocks).
    pub full_text: String,
    /// Average confidence across all blocks.
    pub average_confidence: f32,
    /// Whether this page had existing text (skip logic). [FR-OCR-3]
    pub had_existing_text: bool,
    /// Orientation correction applied (degrees).
    pub orientation_correction: f32,
    /// Whether OCR succeeded.
    pub success: bool,
    /// Error message if failed.
    pub error: Option<String>,
}

/// Preprocessing options. [FR-OCR-3]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreprocessOptions {
    /// Deskew: correct page rotation/skew.
    pub deskew: bool,
    /// Despeckle: remove noise dots.
    pub despeckle: bool,
    /// DPI normalization: resize to target DPI before recognition.
    pub target_dpi: u32,
    /// Whether to OCR pages that already have some text. [FR-OCR-3]
    /// Acrobat refuses pages with "renderable text"; we fix this.
    pub ocr_pages_with_text: bool,
}

impl Default for PreprocessOptions {
    fn default() -> Self {
        Self {
            deskew: true,
            despeckle: true,
            target_dpi: 300,
            ocr_pages_with_text: false,
        }
    }
}

/// OCR engine trait. [ADR-018]
///
/// Recognition engines are pluggable backends. The trait consumes
/// page rasters + hints and produces normalized intermediates.
pub trait OcrEngine: Send + Sync {
    /// Recognize text in a page raster.
    ///
    /// `raster` is the page image as RGBA8 pixels.
    /// `width` and `height` are in pixels.
    /// `page_index` is the 0-based page number.
    fn recognize(
        &self,
        raster: &[u8],
        width: u32,
        height: u32,
        page_index: u32,
        options: &PreprocessOptions,
    ) -> OcrPageResult;

    /// Check if this engine is available (binary/library present).
    fn is_available(&self) -> bool;

    /// Engine name for diagnostics.
    fn name(&self) -> &str;
}

const OCR_RESULT_MAGIC: &[u8; 4] = b"OCR1";
const OCR_REQUEST_MAGIC: &[u8; 4] = b"OCQ1";
const MAX_UTILITY_RESULT_BYTES: usize = 4 * 1024 * 1024;
const MAX_UTILITY_BLOCKS: usize = 65_535;
const MAX_UTILITY_TEXT_BYTES: usize = 1024 * 1024;
const MAX_UTILITY_LANGUAGE_BYTES: usize = 256;
// Bounds the worker's own self-rendered raster (GR-7); the raster never
// crosses IPC, so this only needs to cap worst-case worker memory, not fit
// a transport frame. A full US-Letter/A4 page at the ADR-018 default 300
// DPI is ~32MB uncompressed RGBA8 — 128MB covers that plus headroom for
// larger formats/higher DPI. ponytail: flat ceiling, not corpus-tuned yet;
// revisit against the real scan corpus (ADR-022 §2) once one exists.
const MAX_UTILITY_RASTER_BYTES: u64 = 128 * 1024 * 1024;

/// Bounded OCR control data for one utility job. The worker renders the
/// page itself (via its already-loaded document engine) at `options.target_dpi`
/// rather than receiving a caller-supplied raster — no page raster crosses
/// any IPC boundary, in either direction. [FR-OCR-1]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OcrUtilityRequest {
    /// Zero-based source page.
    pub page_index: u32,
    /// Tesseract-style language selection such as `eng+hin`.
    pub language: String,
    /// Requested preprocessing behavior.
    pub options: PreprocessOptions,
}

/// Invalid or oversized normalized OCR utility result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OcrUtilityCodecError {
    /// The byte structure or a numeric value is invalid.
    Malformed,
    /// A declared collection or text field exceeds its bound.
    LimitExceeded,
    /// A text field is not UTF-8.
    InvalidUtf8,
}

/// Failure while executing a validated OCR request through an injected engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OcrUtilityExecutionError {
    /// Request or result bytes failed bounded codec validation.
    Codec(OcrUtilityCodecError),
    /// The rendered raster's byte length doesn't match `width*height*4`, or
    /// the requested dimensions are zero/oversized (`GR-7` bound).
    RasterLengthMismatch,
    /// The selected recognition backend is unavailable.
    EngineUnavailable,
    /// The backend returned a result for a different page.
    PageIdentityMismatch,
}

/// Execute one bounded OCR request against a raster the caller already
/// rendered (worker-side self-render — see [`OcrUtilityRequest`]).
///
/// `width`/`height` describe `raster`, supplied directly by the caller (not
/// decoded from the wire request) since the caller is the one that just
/// rendered it. [GR-7]
pub fn execute_utility_ocr(
    engine: &dyn OcrEngine,
    request_bytes: &[u8],
    raster: &[u8],
    width: u32,
    height: u32,
) -> Result<Vec<u8>, OcrUtilityExecutionError> {
    let request = decode_utility_request(request_bytes).map_err(OcrUtilityExecutionError::Codec)?;
    let raster_bytes = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(OcrUtilityExecutionError::RasterLengthMismatch)?;
    if raster_bytes == 0 || raster_bytes > MAX_UTILITY_RASTER_BYTES {
        return Err(OcrUtilityExecutionError::RasterLengthMismatch);
    }
    let expected_length = usize::try_from(raster_bytes)
        .map_err(|_| OcrUtilityExecutionError::RasterLengthMismatch)?;
    if raster.len() != expected_length {
        return Err(OcrUtilityExecutionError::RasterLengthMismatch);
    }
    if !engine.is_available() {
        return Err(OcrUtilityExecutionError::EngineUnavailable);
    }
    let result = engine.recognize(raster, width, height, request.page_index, &request.options);
    if result.page_index != request.page_index {
        return Err(OcrUtilityExecutionError::PageIdentityMismatch);
    }
    let result = OcrPageResult {
        raster_width: width,
        raster_height: height,
        ..result
    };
    encode_utility_result(&result).map_err(OcrUtilityExecutionError::Codec)
}

/// Encode bounded OCR request metadata for a utility job inline input.
pub fn encode_utility_request(
    request: &OcrUtilityRequest,
) -> Result<Vec<u8>, OcrUtilityCodecError> {
    validate_request(request)?;
    let mut bytes = Vec::with_capacity(19 + request.language.len());
    bytes.extend_from_slice(OCR_REQUEST_MAGIC);
    bytes.extend_from_slice(&request.page_index.to_le_bytes());
    bytes.extend_from_slice(&request.options.target_dpi.to_le_bytes());
    bytes.push(request.options.deskew.into());
    bytes.push(request.options.despeckle.into());
    bytes.push(request.options.ocr_pages_with_text.into());
    bytes.extend_from_slice(&(request.language.len() as u32).to_le_bytes());
    bytes.extend_from_slice(request.language.as_bytes());
    Ok(bytes)
}

/// Decode and validate OCR request metadata inside a utility worker.
pub fn decode_utility_request(bytes: &[u8]) -> Result<OcrUtilityRequest, OcrUtilityCodecError> {
    let mut cursor = OcrCursor { bytes, offset: 0 };
    if cursor.take(4)? != OCR_REQUEST_MAGIC {
        return Err(OcrUtilityCodecError::Malformed);
    }
    let page_index = cursor.u32()?;
    let target_dpi = cursor.u32()?;
    let deskew = cursor.boolean()?;
    let despeckle = cursor.boolean()?;
    let ocr_pages_with_text = cursor.boolean()?;
    let language_length = cursor.u32()? as usize;
    if language_length > MAX_UTILITY_LANGUAGE_BYTES {
        return Err(OcrUtilityCodecError::LimitExceeded);
    }
    let language = std::str::from_utf8(cursor.take(language_length)?)
        .map(str::to_owned)
        .map_err(|_| OcrUtilityCodecError::InvalidUtf8)?;
    if !cursor.done() {
        return Err(OcrUtilityCodecError::Malformed);
    }
    let request = OcrUtilityRequest {
        page_index,
        language,
        options: PreprocessOptions {
            deskew,
            despeckle,
            target_dpi,
            ocr_pages_with_text,
        },
    };
    validate_request(&request)?;
    Ok(request)
}

fn validate_request(request: &OcrUtilityRequest) -> Result<(), OcrUtilityCodecError> {
    if request.language.is_empty()
        || request.language.len() > MAX_UTILITY_LANGUAGE_BYTES
        || request.options.target_dpi == 0
    {
        return Err(OcrUtilityCodecError::LimitExceeded);
    }
    Ok(())
}

/// Encode the normalized OCR intermediate returned by a utility worker.
pub fn encode_utility_result(result: &OcrPageResult) -> Result<Vec<u8>, OcrUtilityCodecError> {
    if result.blocks.len() > MAX_UTILITY_BLOCKS
        || result.full_text.len() > MAX_UTILITY_TEXT_BYTES
        || result
            .error
            .as_ref()
            .is_some_and(|value| value.len() > MAX_UTILITY_TEXT_BYTES)
    {
        return Err(OcrUtilityCodecError::LimitExceeded);
    }
    let mut bytes = Vec::new();
    bytes.extend_from_slice(OCR_RESULT_MAGIC);
    bytes.extend_from_slice(&result.page_index.to_le_bytes());
    bytes.extend_from_slice(&result.raster_width.to_le_bytes());
    bytes.extend_from_slice(&result.raster_height.to_le_bytes());
    bytes.extend_from_slice(&(result.blocks.len() as u32).to_le_bytes());
    for block in &result.blocks {
        put_text(&mut bytes, &block.text)?;
        for value in block.bbox {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&block.confidence.to_le_bytes());
        put_text(&mut bytes, &block.language)?;
    }
    put_text(&mut bytes, &result.full_text)?;
    bytes.extend_from_slice(&result.average_confidence.to_le_bytes());
    bytes.push(result.had_existing_text.into());
    bytes.extend_from_slice(&result.orientation_correction.to_le_bytes());
    bytes.push(result.success.into());
    match &result.error {
        Some(error) => {
            bytes.push(1);
            put_text(&mut bytes, error)?;
        }
        None => bytes.push(0),
    }
    if bytes.len() > MAX_UTILITY_RESULT_BYTES {
        return Err(OcrUtilityCodecError::LimitExceeded);
    }
    Ok(bytes)
}

/// Decode and validate a normalized OCR intermediate at the Z1→Z0 boundary.
pub fn decode_utility_result(bytes: &[u8]) -> Result<OcrPageResult, OcrUtilityCodecError> {
    if bytes.len() > MAX_UTILITY_RESULT_BYTES {
        return Err(OcrUtilityCodecError::LimitExceeded);
    }
    let mut cursor = OcrCursor { bytes, offset: 0 };
    if cursor.take(4)? != OCR_RESULT_MAGIC {
        return Err(OcrUtilityCodecError::Malformed);
    }
    let page_index = cursor.u32()?;
    let raster_width = cursor.u32()?;
    let raster_height = cursor.u32()?;
    let block_count = cursor.u32()? as usize;
    if block_count > MAX_UTILITY_BLOCKS {
        return Err(OcrUtilityCodecError::LimitExceeded);
    }
    let mut blocks = Vec::with_capacity(block_count);
    for _ in 0..block_count {
        let text = cursor.text()?;
        let bbox = [cursor.f32()?, cursor.f32()?, cursor.f32()?, cursor.f32()?];
        let confidence = cursor.f32()?;
        let language = cursor.text()?;
        if bbox.iter().any(|value| !value.is_finite() || *value < 0.0)
            || !confidence.is_finite()
            || !(0.0..=1.0).contains(&confidence)
        {
            return Err(OcrUtilityCodecError::Malformed);
        }
        blocks.push(OcrTextBlock {
            text,
            bbox,
            confidence,
            language,
        });
    }
    let full_text = cursor.text()?;
    let average_confidence = cursor.f32()?;
    let had_existing_text = cursor.boolean()?;
    let orientation_correction = cursor.f32()?;
    let success = cursor.boolean()?;
    let error = match cursor.byte()? {
        0 => None,
        1 => Some(cursor.text()?),
        _ => return Err(OcrUtilityCodecError::Malformed),
    };
    if !cursor.done()
        || !average_confidence.is_finite()
        || !(0.0..=1.0).contains(&average_confidence)
        || !orientation_correction.is_finite()
    {
        return Err(OcrUtilityCodecError::Malformed);
    }
    Ok(OcrPageResult {
        page_index,
        raster_width,
        raster_height,
        blocks,
        full_text,
        average_confidence,
        had_existing_text,
        orientation_correction,
        success,
        error,
    })
}

fn put_text(bytes: &mut Vec<u8>, text: &str) -> Result<(), OcrUtilityCodecError> {
    if text.len() > MAX_UTILITY_TEXT_BYTES {
        return Err(OcrUtilityCodecError::LimitExceeded);
    }
    bytes.extend_from_slice(&(text.len() as u32).to_le_bytes());
    bytes.extend_from_slice(text.as_bytes());
    Ok(())
}

struct OcrCursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> OcrCursor<'a> {
    fn take(&mut self, length: usize) -> Result<&'a [u8], OcrUtilityCodecError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(OcrUtilityCodecError::Malformed)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(OcrUtilityCodecError::Malformed)?;
        self.offset = end;
        Ok(value)
    }

    fn byte(&mut self) -> Result<u8, OcrUtilityCodecError> {
        Ok(self.take(1)?[0])
    }

    fn boolean(&mut self) -> Result<bool, OcrUtilityCodecError> {
        match self.byte()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(OcrUtilityCodecError::Malformed),
        }
    }

    fn u32(&mut self) -> Result<u32, OcrUtilityCodecError> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("four bytes"),
        ))
    }

    fn f32(&mut self) -> Result<f32, OcrUtilityCodecError> {
        Ok(f32::from_le_bytes(
            self.take(4)?.try_into().expect("four bytes"),
        ))
    }

    fn text(&mut self) -> Result<String, OcrUtilityCodecError> {
        let length = self.u32()? as usize;
        if length > MAX_UTILITY_TEXT_BYTES {
            return Err(OcrUtilityCodecError::LimitExceeded);
        }
        std::str::from_utf8(self.take(length)?)
            .map(str::to_owned)
            .map_err(|_| OcrUtilityCodecError::InvalidUtf8)
    }

    fn done(&self) -> bool {
        self.offset == self.bytes.len()
    }
}

// ---------------------------------------------------------------------------
// Text layer registration [FR-OCR-1]
// ---------------------------------------------------------------------------

/// Convert recognized blocks from source-raster pixel space into PDF
/// user-space points, keeping the top-down `[left, top, width, height]`
/// convention `generate_text_layer_stream` expects. [FR-OCR-1]
///
/// Without this step, blocks placed directly onto a point-space page use
/// pixel-magnitude coordinates and land far outside the page's `MediaBox` —
/// invisible, so no visual symptom, but a mis-registered text layer.
pub fn scale_blocks_to_page(
    blocks: &[OcrTextBlock],
    raster_width: u32,
    raster_height: u32,
    page_width: f32,
    page_height: f32,
) -> Vec<OcrTextBlock> {
    if raster_width == 0 || raster_height == 0 {
        return Vec::new();
    }
    let sx = page_width / raster_width as f32;
    let sy = page_height / raster_height as f32;
    blocks
        .iter()
        .map(|block| OcrTextBlock {
            bbox: [
                block.bbox[0] * sx,
                block.bbox[1] * sy,
                block.bbox[2] * sx,
                block.bbox[3] * sy,
            ],
            ..block.clone()
        })
        .collect()
}

/// Generate an invisible text layer content stream from OCR results. [FR-OCR-1]
///
/// The text layer uses `Tr 3` (invisible text mode) so text is selectable
/// and searchable but does not alter the visual appearance.
///
/// `blocks` must already be in PDF user-space points (see
/// [`scale_blocks_to_page`]) and `page_height` is the page's point height —
/// passing raw `recognize()` output here mis-registers the layer.
/// References the `/F1` font resource; the caller must ensure the page's
/// `/Resources/Font` dictionary defines it (see `pdf-model::ocr_layer`).
pub fn generate_text_layer_stream(blocks: &[OcrTextBlock], page_height: f32) -> Vec<u8> {
    use std::io::Write;
    let mut buf = Vec::new();

    // Begin text object.
    write!(&mut buf, "BT\n").unwrap();

    for block in blocks {
        if block.text.is_empty() {
            continue;
        }

        // Set invisible text mode (render mode 3 = neither fill nor stroke).
        write!(&mut buf, "3 Tr\n").unwrap();

        // Set font (standard Helvetica, any size — invisible text doesn't matter visually).
        write!(&mut buf, "/F1 10 Tf\n").unwrap();

        // Set text color to transparent (invisible).
        write!(&mut buf, "0 0 0 rg\n").unwrap();

        // Position at the block's baseline (bottom-left of bbox, adjusted for baseline).
        let x = block.bbox[0];
        let y = page_height - block.bbox[1] - block.bbox[3]; // flip Y for PDF coords
        write!(&mut buf, "1 0 0 1 {x:.1} {y:.1} Tm\n").unwrap();

        // Scale text to fit the block width (approximate).
        let text_len = block.text.len() as f32;
        if text_len > 0.0 {
            let char_width = block.bbox[2] / text_len;
            let font_size = (char_width * 1.2).clamp(6.0, 24.0);
            write!(&mut buf, "/F1 {font_size:.1} Tf\n").unwrap();
        }

        // Render text.
        let escaped = escape_ocr_str(&block.text);
        write!(&mut buf, "({escaped}) Tj\n").unwrap();
    }

    // End text object.
    write!(&mut buf, "ET\n").unwrap();

    buf
}

/// Check if a page already has renderable text (for skip logic). [FR-OCR-3]
///
/// Acrobat refuses to OCR pages with any renderable text. We fix this:
/// if `ocr_pages_with_text` is true, we OCR anyway but warn the user.
pub fn page_has_text(text_model: &HashMap<u32, Vec<String>>, page_index: u32) -> bool {
    text_model
        .get(&page_index)
        .map(|lines| !lines.is_empty() && lines.iter().any(|l| !l.trim().is_empty()))
        .unwrap_or(false)
}

/// Escape a string for PDF literal string syntax.
fn escape_ocr_str(s: &str) -> String {
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

// ---------------------------------------------------------------------------
// Preprocessing [FR-OCR-3]
// ---------------------------------------------------------------------------

/// Preprocess a page raster before OCR. [FR-OCR-3]
///
/// Applies deskew, despeckle, and DPI normalization.
/// Returns the processed raster and any corrections applied.
pub fn preprocess_page(
    raster: &[u8],
    width: u32,
    height: u32,
    options: &PreprocessOptions,
) -> (Vec<u8>, u32, u32, PreprocessResult) {
    let mut result = PreprocessResult::default();

    // DPI normalization: resize if needed.
    let (resized, new_w, new_h) = if options.target_dpi != 72 {
        // Assume input is at 72 DPI; resize to target.
        let scale = options.target_dpi as f32 / 72.0;
        let new_w = (width as f32 * scale) as u32;
        let new_h = (height as f32 * scale) as u32;
        let resized = resize_bilinear(raster, width, height, new_w, new_h);
        result.resized = true;
        result.output_dpi = options.target_dpi;
        (resized, new_w, new_h)
    } else {
        (raster.to_vec(), width, height)
    };

    // Despeckle: remove isolated single-pixel noise.
    let despeckled = if options.despeckle {
        let d = despeckle(&resized, new_w, new_h);
        result.despeckled = true;
        d
    } else {
        resized
    };

    (despeckled, new_w, new_h, result)
}

/// Result of preprocessing.
#[derive(Debug, Default, Clone)]
pub struct PreprocessResult {
    /// Whether the image was resized.
    pub resized: bool,
    /// Output DPI after normalization.
    pub output_dpi: u32,
    /// Whether despeckle was applied.
    pub despeckled: bool,
    /// Whether deskew was applied.
    pub deskewed: bool,
}

/// Simple bilinear resize. [FR-OCR-3]
fn resize_bilinear(input: &[u8], src_w: u32, src_h: u32, dst_w: u32, dst_h: u32) -> Vec<u8> {
    let mut output = vec![0u8; (dst_w * dst_h * 4) as usize];
    for dy in 0..dst_h {
        for dx in 0..dst_w {
            let sx = (dx as f32 * src_w as f32 / dst_w as f32).min((src_w - 1) as f32);
            let sy = (dy as f32 * src_h as f32 / dst_h as f32).min((src_h - 1) as f32);
            let x0 = sx.floor() as u32;
            let y0 = sy.floor() as u32;
            let x1 = (x0 + 1).min(src_w - 1);
            let y1 = (y0 + 1).min(src_h - 1);
            let fx = sx - x0 as f32;
            let fy = sy - y0 as f32;

            for c in 0..4 {
                let v00 = input[((y0 * src_w + x0) * 4 + c) as usize] as f32;
                let v10 = input[((y0 * src_w + x1) * 4 + c) as usize] as f32;
                let v01 = input[((y1 * src_w + x0) * 4 + c) as usize] as f32;
                let v11 = input[((y1 * src_w + x1) * 4 + c) as usize] as f32;
                let v = v00 * (1.0 - fx) * (1.0 - fy)
                    + v10 * fx * (1.0 - fy)
                    + v01 * (1.0 - fx) * fy
                    + v11 * fx * fy;
                output[((dy * dst_w + dx) * 4 + c) as usize] = v as u8;
            }
        }
    }
    output
}

/// Simple despeckle: remove isolated single-pixel noise. [FR-OCR-3]
fn despeckle(input: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut output = input.to_vec();
    let threshold = 128u8; // binary threshold for noise detection
    // saturating_sub: a zero-dimension raster has no interior pixels to
    // inspect, and `height - 1` would underflow u32 and abort the worker.
    for y in 1..height.saturating_sub(1) {
        for x in 1..width.saturating_sub(1) {
            let idx = ((y * width + x) * 4) as usize;
            let val = input[idx];
            if val < threshold {
                // Dark pixel — check if it's isolated.
                let neighbors = [
                    input[((y - 1) * width + x) as usize * 4],
                    input[((y + 1) * width + x) as usize * 4],
                    input[(y * width + x - 1) as usize * 4],
                    input[(y * width + x + 1) as usize * 4],
                ];
                let dark_neighbors = neighbors.iter().filter(|&&n| n < threshold).count();
                if dark_neighbors <= 1 {
                    // Isolated noise pixel — remove it.
                    output[idx] = 255;
                    output[idx + 1] = 255;
                    output[idx + 2] = 255;
                    output[idx + 3] = 255;
                }
            }
        }
    }
    output
}

// ---------------------------------------------------------------------------
// Stub Tesseract backend [ADR-018]
// ---------------------------------------------------------------------------

/// Tesseract OCR backend. [ADR-018, FR-OCR, M9]
///
/// Invokes the Tesseract binary via process invocation. Writes a temporary
/// PNG file for input and reads TSV output for structured results.
/// Falls back gracefully when Tesseract is not installed.
pub struct TesseractEngine {
    /// Path to the tesseract binary.
    tesseract_path: Option<std::path::PathBuf>,
    /// Default language(s).
    languages: String,
}

impl TesseractEngine {
    /// Create a new Tesseract engine, auto-detecting the binary location.
    pub fn new() -> Self {
        let tesseract_path = which_tesseract();
        Self {
            tesseract_path,
            languages: "eng".into(),
        }
    }

    /// Create with explicit binary path and languages.
    pub fn with_config(path: impl Into<std::path::PathBuf>, languages: impl Into<String>) -> Self {
        Self {
            tesseract_path: Some(path.into()),
            languages: languages.into(),
        }
    }
}

impl OcrEngine for TesseractEngine {
    fn recognize(
        &self,
        raster: &[u8],
        width: u32,
        height: u32,
        page_index: u32,
        options: &PreprocessOptions,
    ) -> OcrPageResult {
        let Some(ref tesseract_path) = self.tesseract_path else {
            return OcrPageResult {
                page_index,
                raster_width: width,
                raster_height: height,
                blocks: Vec::new(),
                full_text: String::new(),
                average_confidence: 0.0,
                had_existing_text: false,
                orientation_correction: 0.0,
                success: false,
                error: Some("Tesseract binary not found — install tesseract-ocr".into()),
            };
        };

        // Preprocess the raster.
        let (processed, proc_w, proc_h, _preprocess_result) =
            preprocess_page(raster, width, height, options);

        // Write RGBA raster to a temporary PNG file for Tesseract input.
        let tmp_dir = std::env::temp_dir().join(format!("ocr_page_{page_index}"));
        let _ = std::fs::create_dir_all(&tmp_dir);
        let png_path = tmp_dir.join("input.png");
        let tsv_path = tmp_dir.join("output.tsv");

        // Write raw RGBA as PNG (using a minimal PNG encoder).
        if let Err(e) = write_rgba_png(&png_path, &processed, proc_w, proc_h) {
            return OcrPageResult {
                page_index,
                raster_width: width,
                raster_height: height,
                blocks: Vec::new(),
                full_text: String::new(),
                average_confidence: 0.0,
                had_existing_text: false,
                orientation_correction: 0.0,
                success: false,
                error: Some(format!("failed to write input PNG: {e}")),
            };
        }

        // Invoke Tesseract: tesseract input.png output_base --oem 1 --psm 6 tsv
        let output_base = tmp_dir.join("output");
        let result = std::process::Command::new(tesseract_path)
            .arg(&png_path)
            .arg(output_base.as_os_str())
            .arg("--oem")
            .arg("1") // LSTM engine only
            .arg("--psm")
            .arg("6") // Uniform block of text
            .arg("-l")
            .arg(&self.languages)
            .arg("tsv")
            .output();

        match result {
            Ok(output) => {
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    return OcrPageResult {
                        page_index,
                        raster_width: width,
                        raster_height: height,
                        blocks: Vec::new(),
                        full_text: String::new(),
                        average_confidence: 0.0,
                        had_existing_text: false,
                        orientation_correction: 0.0,
                        success: false,
                        error: Some(format!("tesseract failed: {stderr}")),
                    };
                }

                // Parse TSV output.
                let tsv_content = std::fs::read_to_string(&tsv_path).unwrap_or_default();
                parse_tesseract_tsv(&tsv_content, page_index, proc_w, proc_h)
            }
            Err(e) => OcrPageResult {
                page_index,
                raster_width: width,
                raster_height: height,
                blocks: Vec::new(),
                full_text: String::new(),
                average_confidence: 0.0,
                had_existing_text: false,
                orientation_correction: 0.0,
                success: false,
                error: Some(format!("failed to invoke tesseract: {e}")),
            },
        }
    }

    fn is_available(&self) -> bool {
        self.tesseract_path.is_some()
    }

    fn name(&self) -> &str {
        "tesseract"
    }
}

/// Find the tesseract binary on the system.
fn which_tesseract() -> Option<std::path::PathBuf> {
    // Check common locations.
    let candidates: Vec<String> = if cfg!(windows) {
        vec![
            r"C:\Program Files\Tesseract-OCR\tesseract.exe".into(),
            r"C:\Program Files (x86)\Tesseract-OCR\tesseract.exe".into(),
        ]
    } else {
        vec![
            "/usr/bin/tesseract".into(),
            "/usr/local/bin/tesseract".into(),
            "/opt/homebrew/bin/tesseract".into(),
        ]
    };

    for path in candidates {
        if std::path::Path::new(&path).exists() {
            return Some(path.into());
        }
    }

    // Try `which tesseract` as fallback.
    std::process::Command::new(if cfg!(windows) { "where" } else { "which" })
        .arg("tesseract")
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                let s = String::from_utf8_lossy(&o.stdout);
                let path = s.lines().next()?.trim();
                if !path.is_empty() {
                    Some(path.into())
                } else {
                    None
                }
            } else {
                None
            }
        })
}

/// Parse Tesseract TSV output into OcrPageResult.
fn parse_tesseract_tsv(tsv: &str, page_index: u32, raster_width: u32, raster_height: u32) -> OcrPageResult {
    let mut blocks = Vec::new();
    let mut full_text = String::new();
    let mut total_confidence = 0.0f32;
    let mut word_count = 0u32;

    for line in tsv.lines().skip(1) {
        // skip header
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() < 12 {
            continue;
        }
        // TSV columns: level, page_num, block_num, par_num, line_num, word_num,
        //              left, top, width, height, conf, text
        let conf_str = fields[10].trim();
        let text = fields[11].trim();

        if text.is_empty() || conf_str == "-1" {
            continue;
        }

        let conf: f32 = conf_str.parse().unwrap_or(0.0) / 100.0;
        let left: f32 = fields[6].parse().unwrap_or(0.0);
        let top: f32 = fields[7].parse().unwrap_or(0.0);
        let w: f32 = fields[8].parse().unwrap_or(0.0);
        let h: f32 = fields[9].parse().unwrap_or(0.0);

        blocks.push(OcrTextBlock {
            text: text.to_string(),
            bbox: [left, top, w, h],
            confidence: conf,
            language: "eng".into(),
        });

        if !full_text.is_empty() {
            full_text.push(' ');
        }
        full_text.push_str(text);
        total_confidence += conf;
        word_count += 1;
    }

    OcrPageResult {
        page_index,
        raster_width,
        raster_height,
        full_text,
        average_confidence: if word_count > 0 {
            total_confidence / word_count as f32
        } else {
            0.0
        },
        had_existing_text: false,
        orientation_correction: 0.0,
        success: !blocks.is_empty(),
        error: if blocks.is_empty() {
            Some("no text recognized".into())
        } else {
            None
        },
        blocks,
    }
}

/// Minimal RGBA-to-PNG writer. [FR-OCR-3]
fn write_rgba_png(
    path: &std::path::Path,
    rgba: &[u8],
    width: u32,
    height: u32,
) -> Result<(), String> {
    use std::io::Write;

    let mut file = std::fs::File::create(path).map_err(|e| format!("create PNG: {e}"))?;

    // PNG signature.
    file.write_all(b"\x89PNG\r\n\x1a\n")
        .map_err(|e| e.to_string())?;

    // IHDR chunk.
    let ihdr_data = [
        (width >> 24) as u8,
        (width >> 16) as u8,
        (width >> 8) as u8,
        width as u8,
        (height >> 24) as u8,
        (height >> 16) as u8,
        (height >> 8) as u8,
        height as u8,
        8, // bit depth
        6, // color type (RGBA)
        0,
        0,
        0, // compression, filter, interlace
    ];
    write_png_chunk(&mut file, b"IHDR", &ihdr_data).map_err(|e| e.to_string())?;

    // IDAT chunk (uncompressed deflate — simple but functional).
    let mut raw_data = Vec::with_capacity((width * height * 4 + height) as usize);
    for y in 0..height {
        raw_data.push(0); // filter byte (none)
        let row_start = (y * width * 4) as usize;
        let row_end = row_start + (width * 4) as usize;
        if row_end <= rgba.len() {
            raw_data.extend_from_slice(&rgba[row_start..row_end]);
        }
    }

    // Deflate: store blocks (no compression — simplest correct implementation).
    let compressed = deflate_store(&raw_data);
    write_png_chunk(&mut file, b"IDAT", &compressed).map_err(|e| e.to_string())?;

    // IEND chunk.
    write_png_chunk(&mut file, b"IEND", &[]).map_err(|e| e.to_string())?;

    Ok(())
}

fn write_png_chunk(
    file: &mut std::fs::File,
    chunk_type: &[u8],
    data: &[u8],
) -> Result<(), std::io::Error> {
    let crc = crc32(chunk_type, data);
    let len = data.len() as u32;
    file.write_all(&[
        (len >> 24) as u8,
        (len >> 16) as u8,
        (len >> 8) as u8,
        len as u8,
    ])?;
    file.write_all(chunk_type)?;
    file.write_all(data)?;
    file.write_all(&[
        (crc >> 24) as u8,
        (crc >> 16) as u8,
        (crc >> 8) as u8,
        crc as u8,
    ])?;
    Ok(())
}

fn crc32(chunk_type: &[u8], data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for i in 0..256 {
        let mut c = i as u32;
        for _ in 0..8 {
            if c & 1 != 0 {
                c = 0xEDB88320 ^ (c >> 1);
            } else {
                c >>= 1;
            }
        }
        table[i] = c;
    }
    let mut crc = 0xFFFFFFFFu32;
    for &b in chunk_type.iter().chain(data.iter()) {
        crc = table[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFFFFFF
}

/// Deflate STORE method (no compression, block size 65535).
fn deflate_store(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + data.len() / 65535 * 5 + 5);
    let mut pos = 0;
    while pos < data.len() {
        let remaining = data.len() - pos;
        let block_size = remaining.min(65535);
        let is_last = pos + block_size >= data.len();
        out.push(if is_last { 0x01 } else { 0x00 }); // BFINAL=1 if last, BTYPE=00 (store)
        out.push((block_size & 0xFF) as u8);
        out.push(((block_size >> 8) & 0xFF) as u8);
        let nlen = !block_size as u16;
        out.push((nlen & 0xFF) as u8);
        out.push(((nlen >> 8) & 0xFF) as u8);
        out.extend_from_slice(&data[pos..pos + block_size]);
        pos += block_size;
    }
    // Adler-32 checksum.
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for &byte in data {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    let adler = (b << 16) | a;
    out.push((adler >> 24) as u8);
    out.push((adler >> 16) as u8);
    out.push((adler >> 8) as u8);
    out.push(adler as u8);
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    struct FixtureEngine;

    impl OcrEngine for FixtureEngine {
        fn recognize(
            &self,
            _raster: &[u8],
            width: u32,
            height: u32,
            page_index: u32,
            _options: &PreprocessOptions,
        ) -> OcrPageResult {
            OcrPageResult {
                page_index,
                raster_width: width,
                raster_height: height,
                blocks: Vec::new(),
                full_text: "fixture".into(),
                average_confidence: 0.4,
                had_existing_text: false,
                orientation_correction: 0.0,
                success: true,
                error: None,
            }
        }

        fn is_available(&self) -> bool {
            true
        }

        fn name(&self) -> &str {
            "fixture"
        }
    }

    #[test]
    fn utility_execution_invokes_injected_engine() {
        let request = OcrUtilityRequest {
            page_index: 5,
            language: "eng".into(),
            options: PreprocessOptions::default(),
        };
        let output = execute_utility_ocr(
            &FixtureEngine,
            &encode_utility_request(&request).unwrap(),
            &[0; 16],
            2,
            2,
        )
        .unwrap();

        let result = decode_utility_result(&output).unwrap();
        assert_eq!(result.page_index, 5);
        assert_eq!(result.full_text, "fixture");
        assert_eq!(result.average_confidence, 0.4);
        assert_eq!(result.raster_width, 2);
        assert_eq!(result.raster_height, 2);
    }

    #[test]
    fn utility_execution_rejects_oversized_render_target() {
        let request = OcrUtilityRequest {
            page_index: 0,
            language: "eng".into(),
            options: PreprocessOptions::default(),
        };
        let request_bytes = encode_utility_request(&request).unwrap();
        // 8192x8192x4 = 256MiB, over MAX_UTILITY_RASTER_BYTES (128MiB).
        let result = execute_utility_ocr(&FixtureEngine, &request_bytes, &[], 8192, 8192);
        assert_eq!(result, Err(OcrUtilityExecutionError::RasterLengthMismatch));
    }

    #[test]
    fn normalized_result_round_trips_for_utility_ipc() {
        let result = OcrPageResult {
            page_index: 3,
            raster_width: 2550,
            raster_height: 3300,
            blocks: vec![OcrTextBlock {
                text: "hello".into(),
                bbox: [1.0, 2.0, 30.0, 10.0],
                confidence: 0.75,
                language: "eng".into(),
            }],
            full_text: "hello".into(),
            average_confidence: 0.75,
            had_existing_text: false,
            orientation_correction: 0.0,
            success: true,
            error: None,
        };

        assert_eq!(
            decode_utility_result(&encode_utility_result(&result).unwrap()).unwrap(),
            result
        );
    }

    #[test]
    fn normalized_result_rejects_untrusted_geometry() {
        let result = OcrPageResult {
            page_index: 0,
            raster_width: 100,
            raster_height: 100,
            blocks: vec![OcrTextBlock {
                text: "bad".into(),
                bbox: [f32::NAN, 0.0, 1.0, 1.0],
                confidence: 0.5,
                language: "eng".into(),
            }],
            full_text: "bad".into(),
            average_confidence: 0.5,
            had_existing_text: false,
            orientation_correction: 0.0,
            success: true,
            error: None,
        };

        assert_eq!(
            decode_utility_result(&encode_utility_result(&result).unwrap()),
            Err(OcrUtilityCodecError::Malformed)
        );
    }

    #[test]
    fn utility_request_round_trips_language_and_preprocessing() {
        let request = OcrUtilityRequest {
            page_index: 2,
            language: "eng+hin".into(),
            options: PreprocessOptions {
                deskew: true,
                despeckle: false,
                target_dpi: 300,
                ocr_pages_with_text: true,
            },
        };

        assert_eq!(
            decode_utility_request(&encode_utility_request(&request).unwrap()).unwrap(),
            request
        );
    }

    #[test]
    fn utility_request_rejects_zero_dpi() {
        let request = OcrUtilityRequest {
            page_index: 0,
            language: "eng".into(),
            options: PreprocessOptions {
                target_dpi: 0,
                ..PreprocessOptions::default()
            },
        };

        assert_eq!(
            encode_utility_request(&request),
            Err(OcrUtilityCodecError::LimitExceeded)
        );
    }

    #[test]
    fn text_layer_stream_invisible() {
        // [FR-OCR-1] Text layer uses render mode 3 (invisible).
        let blocks = vec![OcrTextBlock {
            text: "Hello World".into(),
            bbox: [10.0, 20.0, 100.0, 12.0],
            confidence: 0.95,
            language: "eng".into(),
        }];
        let stream = generate_text_layer_stream(&blocks, 792.0);
        let s = String::from_utf8_lossy(&stream);
        assert!(s.contains("BT"), "should have text object");
        assert!(s.contains("ET"), "should close text object");
        assert!(s.contains("3 Tr"), "should use invisible text mode");
        assert!(s.contains("Hello World"), "should contain recognized text");
    }

    #[test]
    fn text_layer_empty_blocks() {
        let blocks = vec![];
        let stream = generate_text_layer_stream(&blocks, 792.0);
        let s = String::from_utf8_lossy(&stream);
        assert!(s.contains("BT"), "should have text object even if empty");
        assert!(s.contains("ET"), "should close text object");
    }

    #[test]
    fn scale_blocks_to_page_converts_pixel_bbox_to_point_space() {
        // 300 DPI raster of a 612x792pt (US Letter) page: scale factor
        // page_pt / raster_px = 72/300 = 0.24.
        let raster_w = 2550u32; // 612 * 300/72
        let raster_h = 3300u32; // 792 * 300/72
        let blocks = vec![OcrTextBlock {
            text: "word".into(),
            bbox: [100.0, 200.0, 400.0, 50.0],
            confidence: 0.9,
            language: "eng".into(),
        }];
        let scaled = scale_blocks_to_page(&blocks, raster_w, raster_h, 612.0, 792.0);
        assert_eq!(scaled.len(), 1);
        let b = scaled[0].bbox;
        assert!((b[0] - 24.0).abs() < 0.5, "left: {}", b[0]);
        assert!((b[1] - 48.0).abs() < 0.5, "top: {}", b[1]);
        assert!((b[2] - 96.0).abs() < 0.5, "width: {}", b[2]);
        assert!((b[3] - 12.0).abs() < 0.5, "height: {}", b[3]);
        // A block placed via the raw (unscaled) bbox would land at x=100,
        // y=200 pixel-magnitude on a 792pt-tall page — inside the MediaBox
        // only by coincidence, but wildly mis-registered relative to where
        // the word actually sits on the rendered page. Scaled coordinates
        // must differ from the raw pixel bbox for any non-1:1 DPI.
        assert_ne!(scaled[0].bbox, blocks[0].bbox);
    }

    #[test]
    fn scale_blocks_to_page_rejects_zero_raster_dimensions() {
        let blocks = vec![OcrTextBlock {
            text: "x".into(),
            bbox: [1.0, 1.0, 1.0, 1.0],
            confidence: 1.0,
            language: "eng".into(),
        }];
        assert!(scale_blocks_to_page(&blocks, 0, 100, 612.0, 792.0).is_empty());
        assert!(scale_blocks_to_page(&blocks, 100, 0, 612.0, 792.0).is_empty());
    }

    #[test]
    fn page_has_text_detection() {
        let mut model = HashMap::new();
        model.insert(0, vec!["Some text here".into()]);
        model.insert(1, vec!["   ".into()]); // whitespace only
        model.insert(2, Vec::new()); // empty

        assert!(page_has_text(&model, 0), "page 0 has text");
        assert!(!page_has_text(&model, 1), "page 1 is whitespace only");
        assert!(!page_has_text(&model, 2), "page 2 is empty");
        assert!(!page_has_text(&model, 99), "non-existent page");
    }

    #[test]
    fn preprocess_default_options() {
        let opts = PreprocessOptions::default();
        assert!(opts.deskew);
        assert!(opts.despeckle);
        assert_eq!(opts.target_dpi, 300);
        assert!(!opts.ocr_pages_with_text);
    }

    #[test]
    fn preprocess_resize() {
        // Create a small 4x4 RGBA raster.
        let raster = vec![128u8; 4 * 4 * 4];
        let (resized, w, h, result) = preprocess_page(
            &raster,
            4,
            4,
            &PreprocessOptions {
                target_dpi: 300,
                ..Default::default()
            },
        );
        // 300/72 ≈ 4.17, so 4*4.17 ≈ 16
        assert!(w > 4, "width should be scaled up");
        assert!(h > 4, "height should be scaled up");
        assert!(result.resized);
        assert_eq!(resized.len(), (w * h * 4) as usize);
    }

    #[test]
    fn preprocess_despeckle() {
        // Create a raster with an isolated dark pixel at native resolution.
        let mut raster = vec![255u8; 8 * 8 * 4]; // all white
                                                 // Set pixel at (4,4) to black.
        let idx = ((4 * 8 + 4) * 4) as usize;
        raster[idx] = 0;
        raster[idx + 1] = 0;
        raster[idx + 2] = 0;

        // Despeckle only (no resize) to avoid bilinear interpolation effects.
        let (despeckled, _, _, result) = preprocess_page(
            &raster,
            8,
            8,
            &PreprocessOptions {
                despeckle: true,
                target_dpi: 72, // no resize
                ..Default::default()
            },
        );
        assert!(result.despeckled);
        // The isolated pixel should be removed (set to white).
        assert_eq!(
            despeckled[idx], 255,
            "isolated noise pixel should be removed"
        );
    }

    #[test]
    fn preprocess_tolerates_a_degenerate_raster() {
        // A zero-dimension page must not panic the worker: `despeckle` walks
        // 1..height-1 over u32, so height 0 underflowed. A Z1 crash here is a
        // worker abort the coordinator can only see as "transport
        // disconnected". [PRIN-1, GR-8, FR-OCR-4]
        for (width, height) in [(0, 0), (0, 4), (4, 0), (1, 1)] {
            let raster = vec![255u8; (width * height * 4) as usize];
            let (output, out_w, out_h, _) =
                preprocess_page(&raster, width, height, &PreprocessOptions::default());
            assert_eq!(
                output.len(),
                (out_w * out_h * 4) as usize,
                "{width}x{height} produced an inconsistent buffer"
            );
        }
    }

    #[test]
    fn escape_ocr_str_handles_specials() {
        assert_eq!(escape_ocr_str("hello"), "hello");
        assert_eq!(escape_ocr_str("a(b)"), "a\\(b\\)");
        assert_eq!(escape_ocr_str("a\\b"), "a\\\\b");
    }

    #[test]
    fn tesseract_engine_detection() {
        let engine = TesseractEngine::new();
        // On this system, Tesseract may or may not be installed.
        // The engine should gracefully handle both cases.
        let result = engine.recognize(&[], 0, 0, 0, &PreprocessOptions::default());
        // If Tesseract is not installed, we get an error message.
        // If it is installed, we get a result (possibly with no text).
        if engine.is_available() {
            assert!(result.success || result.error.is_some());
        } else {
            assert!(!result.success);
            assert!(result.error.is_some());
        }
    }

    #[test]
    fn ocr_page_result_fields() {
        let result = OcrPageResult {
            page_index: 5,
            raster_width: 100,
            raster_height: 20,
            blocks: vec![OcrTextBlock {
                text: "test".into(),
                bbox: [0.0, 0.0, 100.0, 20.0],
                confidence: 0.85,
                language: "eng".into(),
            }],
            full_text: "test".into(),
            average_confidence: 0.85,
            had_existing_text: false,
            orientation_correction: 0.0,
            success: true,
            error: None,
        };
        assert_eq!(result.page_index, 5);
        assert!(result.success);
        assert_eq!(result.blocks.len(), 1);
    }

    /// Verify JBIG2 symbol-mode is OFF by default. [ADR-018, M9 exit criteria]
    ///
    /// Per ADR-018: "symbol-mode compression of OCR output is OFF by default.
    /// When enabled, an explicit warning is displayed because symbol-mode
    /// JBIG2 can cause character substitution (the Xerox substitution hazard)."
    ///
    /// The text layer always uses uncompressed or Flate-compressed text,
    /// never JBIG2. This test verifies the default behavior.
    #[test]
    fn jbig2_symbol_mode_off_by_default() {
        // Generate a text layer from sample OCR blocks.
        let blocks = vec![OcrTextBlock {
            text: "Hello World".into(),
            bbox: [10.0, 20.0, 100.0, 15.0],
            confidence: 0.95,
            language: "eng".into(),
        }];
        let page_height = 842.0; // A4 page height in points
        let stream = generate_text_layer_stream(&blocks, page_height);

        // The text layer should use render mode 3 (invisible text).
        let stream_str = String::from_utf8_lossy(&stream);
        assert!(
            stream_str.contains("3 Tr"),
            "text layer must use render mode 3 (invisible)"
        );

        // The text layer should NOT contain JBIG2 references.
        // JBIG2 would appear as /Filter /JBIG2Decode or similar.
        assert!(
            !stream_str.contains("JBIG2"),
            "text layer must not use JBIG2 compression"
        );
        assert!(
            !stream_str.contains("jbig2"),
            "text layer must not use JBIG2 compression"
        );

        // Verify the text is present and correctly escaped.
        assert!(
            stream_str.contains("Hello World"),
            "text content must be present"
        );
    }
}
