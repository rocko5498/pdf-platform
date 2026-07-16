//! Commands: shell/CLI → coordinator → worker. [ADR-031, SDS §5]
//!
//! M0: text-based protocol over framed IPC. Two encoding generations coexist:
//!
//! - **Legacy** (v0): raw byte patterns (`b"inspect"`, `b"quit"`, `b"tile_smoke"`)
//!   and the original `render_tile\nv1\n...` format. Supported for backward
//!   compatibility during the transition.
//! - **Typed** (v1): `CMD:<type>:<correlation_id>\n<key>=<value>\n...` envelope.
//!   Every command carries a correlation ID so the coordinator can match
//!   responses. New code should produce typed frames.
//!
//! The worker accepts both generations during M0; the coordinator produces
//! typed frames exclusively.

use std::fmt;

/// Monotonically increasing identifier for matching requests to responses.
pub type CorrelationId = u64;

// ---------------------------------------------------------------------------
// Typed command enum
// ---------------------------------------------------------------------------

/// A command sent from the coordinator to a worker process. [SDS §4.2, §5.1]
///
/// Every variant carries a `correlation_id` so the coordinator can route
/// the worker's response back to the originating caller.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Render a tile into shared memory. [ADR-007, SDS §6]
    RenderTile {
        /// Correlation ID for response matching.
        correlation_id: CorrelationId,
        /// 0-based page index.
        page: u32,
        /// Device-space X origin.
        x: u32,
        /// Device-space Y origin.
        y: u32,
        /// Width in device pixels.
        w: u32,
        /// Height in device pixels.
        h: u32,
        /// Scale factor.
        scale: f32,
        /// Render generation for invalidation.
        generation: u64,
        /// Byte offset into the shared-memory region where output goes.
        slot_offset: u32,
        /// Column in the tile grid (for multi-tile pages).
        col: u32,
        /// Row in the tile grid (for multi-tile pages).
        row: u32,
    },
    /// Scan the inherited document and return a structural summary. [SDS §3.1]
    Inspect {
        /// Correlation ID for response matching.
        correlation_id: CorrelationId,
    },
    /// Extract text from a page and return the canonical text model. [ADR-019, M2]
    ExtractPage {
        /// Correlation ID for response matching.
        correlation_id: CorrelationId,
        /// 0-based page index.
        page_index: u32,
    },
    /// Query the document outline (bookmarks). [FR-BOOK, M1]
    GetOutline {
        /// Correlation ID for response matching.
        correlation_id: CorrelationId,
    },
    /// Query optional content groups (layers). [FR-LAYER, M1]
    GetLayers {
        /// Correlation ID for response matching.
        correlation_id: CorrelationId,
    },
    /// Query embedded file attachments. [FR-EMB, M1]
    GetAttachments {
        /// Correlation ID for response matching.
        correlation_id: CorrelationId,
    },
    /// Fetch raw bytes of a specific object by number. [SDS §3.1]
    ///
    /// Used by the coordinator to read document structure (e.g., Pages /Kids array,
    /// page /Rotate values) that isn't available in the structural summary.
    GetObject {
        /// Correlation ID for response matching.
        correlation_id: CorrelationId,
        /// 1-based PDF indirect object number.
        obj_num: u32,
    },
    /// Delete pages by 0-based indices. [FR-ORG, M3, SDS §3.3]
    DeletePages {
        /// Correlation ID for response matching.
        correlation_id: CorrelationId,
        /// 0-based page indices to delete (sorted ascending recommended).
        page_indices: Vec<u32>,
    },
    /// Rotate pages by 0-based indices. [FR-ROTATE, M3]
    RotatePages {
        /// Correlation ID for response matching.
        correlation_id: CorrelationId,
        /// 0-based page indices to rotate.
        page_indices: Vec<u32>,
        /// Rotation in degrees (90, 180, 270).
        degrees: u32,
    },
    /// Add an annotation to a page. [FR-ANNOT, M4]
    AddAnnotation {
        /// Correlation ID for response matching.
        correlation_id: CorrelationId,
        /// 0-based page index.
        page_index: u32,
        /// Annotation type as a string (e.g., "highlight", "sticky_note", "ink").
        annotation_type: String,
        /// Rect in PDF user-space: x,y,width,height (comma-separated).
        rect: String,
        /// Optional content text (for sticky notes, free text).
        contents: Option<String>,
        /// Optional color as "r,g,b,a" (0.0-1.0 each).
        color: Option<String>,
    },
    /// Delete an annotation by ID. [FR-ANNOT, M4]
    DeleteAnnotation {
        /// Correlation ID for response matching.
        correlation_id: CorrelationId,
        /// 0-based page index.
        page_index: u32,
        /// Annotation ID to delete.
        annotation_id: u64,
    },
    /// Clean shutdown — worker exits its main loop. [SDS §10.1]
    Quit,
}

impl Command {
    /// Return the correlation ID, if this command carries one.
    pub fn correlation_id(&self) -> Option<CorrelationId> {
        match self {
            Self::RenderTile { correlation_id, .. } => Some(*correlation_id),
            Self::Inspect { correlation_id } => Some(*correlation_id),
            Self::ExtractPage { correlation_id, .. } => Some(*correlation_id),
            Self::GetOutline { correlation_id } => Some(*correlation_id),
            Self::GetLayers { correlation_id } => Some(*correlation_id),
            Self::GetAttachments { correlation_id } => Some(*correlation_id),
            Self::GetObject { correlation_id, .. } => Some(*correlation_id),
            Self::DeletePages { correlation_id, .. } => Some(*correlation_id),
            Self::RotatePages { correlation_id, .. } => Some(*correlation_id),
            Self::AddAnnotation { correlation_id, .. } => Some(*correlation_id),
            Self::DeleteAnnotation { correlation_id, .. } => Some(*correlation_id),
            Self::Quit => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Wire codec — typed envelope
// ---------------------------------------------------------------------------

/// Encode a [`Command`] as a text frame body.
///
/// Wire format: `CMD:<TYPE>:<correlation_id>\n<key>=<value>\n...`
pub fn encode_command(cmd: &Command) -> Vec<u8> {
    match cmd {
        Command::RenderTile {
            correlation_id,
            page,
            x,
            y,
            w,
            h,
            scale,
            generation,
            slot_offset,
            col,
            row,
        } => format!(
            "CMD:RENDER_TILE:{correlation_id}\n\
             page={page}\nx={x}\ny={y}\nw={w}\nh={h}\n\
             scale={scale}\ngeneration={generation}\nslot_offset={slot_offset}\n\
             col={col}\nrow={row}\n"
        )
        .into_bytes(),
        Command::Inspect { correlation_id } => {
            format!("CMD:INSPECT:{correlation_id}\n").into_bytes()
        }
        Command::ExtractPage { correlation_id, page_index } => {
            format!("CMD:EXTRACT_PAGE:{correlation_id}\npage_index={page_index}\n").into_bytes()
        }
        Command::GetOutline { correlation_id } => {
            format!("CMD:GET_OUTLINE:{correlation_id}\n").into_bytes()
        }
        Command::GetLayers { correlation_id } => {
            format!("CMD:GET_LAYERS:{correlation_id}\n").into_bytes()
        }
        Command::GetAttachments { correlation_id } => {
            format!("CMD:GET_ATTACHMENTS:{correlation_id}\n").into_bytes()
        }
        Command::GetObject { correlation_id, obj_num } => {
            format!("CMD:GET_OBJECT:{correlation_id}\nobj_num={obj_num}\n").into_bytes()
        }
        Command::DeletePages { correlation_id, page_indices } => {
            let indices_str = page_indices.iter()
                .map(|i| i.to_string())
                .collect::<Vec<_>>()
                .join(",");
            format!(
                "CMD:DELETE_PAGES:{correlation_id}\n\
                 page_indices={indices_str}\n"
            ).into_bytes()
        }
        Command::RotatePages { correlation_id, page_indices, degrees } => {
            let indices_str = page_indices.iter()
                .map(|i| i.to_string())
                .collect::<Vec<_>>()
                .join(",");
            format!(
                "CMD:ROTATE_PAGES:{correlation_id}\n\
                 page_indices={indices_str}\ndegrees={degrees}\n"
            ).into_bytes()
        }
        Command::AddAnnotation { correlation_id, page_index, annotation_type, rect, contents, color } => {
            let mut out = format!(
                "CMD:ADD_ANNOTATION:{correlation_id}\n\
                 page_index={page_index}\nannotation_type={annotation_type}\nrect={rect}\n"
            );
            if let Some(c) = contents {
                out.push_str(&format!("contents={c}\n"));
            }
            if let Some(clr) = color {
                out.push_str(&format!("color={clr}\n"));
            }
            out.into_bytes()
        }
        Command::DeleteAnnotation { correlation_id, page_index, annotation_id } => {
            format!(
                "CMD:DELETE_ANNOTATION:{correlation_id}\n\
                 page_index={page_index}\nannotation_id={annotation_id}\n"
            ).into_bytes()
        }
        Command::Quit => b"CMD:QUIT\n".to_vec(),
    }
}

/// Errors from decoding a command frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandDecodeError {
    /// Not valid UTF-8.
    InvalidUtf8,
    /// Missing or unknown command type header.
    UnknownCommand,
    /// Missing or invalid field.
    BadField(&'static str),
}

impl fmt::Display for CommandDecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUtf8 => write!(f, "command not utf-8"),
            Self::UnknownCommand => write!(f, "unknown command type"),
            Self::BadField(k) => write!(f, "bad field: {k}"),
        }
    }
}

impl std::error::Error for CommandDecodeError {}

/// Decode a command from a frame body.
///
/// Accepts both the typed `CMD:...` envelope and legacy raw-byte commands
/// for backward compatibility during M0 transition.
pub fn decode_command(body: &[u8]) -> Result<Command, CommandDecodeError> {
    let text = std::str::from_utf8(body).map_err(|_| CommandDecodeError::InvalidUtf8)?;

    // Try typed envelope first.
    if let Some(cmd) = try_decode_typed(text)? {
        return Ok(cmd);
    }

    // Fall through to legacy decoders.
    if text.starts_with("render_tile\n") {
        // Legacy v1 render_tile — wrap in typed command with correlation_id=0.
        let req = decode_render_tile(body)?;
        return Ok(Command::RenderTile {
            correlation_id: 0,
            page: req.page,
            x: req.x,
            y: req.y,
            w: req.w,
            h: req.h,
            scale: req.scale,
            generation: req.generation,
            slot_offset: req.slot_offset,
            col: 0,
            row: 0,
        });
    }

    if text == "inspect" {
        return Ok(Command::Inspect { correlation_id: 0 });
    }

    if text == "quit" {
        return Ok(Command::Quit);
    }

    Err(CommandDecodeError::UnknownCommand)
}

fn try_decode_typed(text: &str) -> Result<Option<Command>, CommandDecodeError> {
    let Some(first_line) = text.lines().next() else {
        return Ok(None);
    };

    // Parse "CMD:<TYPE>:<correlation_id>" or "CMD:<TYPE>" (for Quit)
    let Some(rest) = first_line.strip_prefix("CMD:") else {
        return Ok(None);
    };
    let (type_str, correlation_id) = if let Some((t, id_str)) = rest.split_once(':') {
        let id: CorrelationId = id_str
            .parse()
            .map_err(|_| CommandDecodeError::BadField("correlation_id"))?;
        (t, Some(id))
    } else {
        (rest, None)
    };

    match type_str {
        "RENDER_TILE" => {
            let cid = correlation_id.unwrap_or(0);
            let mut page = None;
            let mut x = None;
            let mut y = None;
            let mut w = None;
            let mut h = None;
            let mut scale = None;
            let mut generation = None;
            let mut slot_offset = None;
            let mut col = None;
            let mut row = None;
            for line in text.lines().skip(1) {
                let Some((k, v)) = line.split_once('=') else {
                    continue;
                };
                match k {
                    "page" => {
                        page = Some(v.parse().map_err(|_| CommandDecodeError::BadField("page"))?)
                    }
                    "x" => {
                        x = Some(v.parse().map_err(|_| CommandDecodeError::BadField("x"))?)
                    }
                    "y" => {
                        y = Some(v.parse().map_err(|_| CommandDecodeError::BadField("y"))?)
                    }
                    "w" => {
                        w = Some(v.parse().map_err(|_| CommandDecodeError::BadField("w"))?)
                    }
                    "h" => {
                        h = Some(v.parse().map_err(|_| CommandDecodeError::BadField("h"))?)
                    }
                    "scale" => {
                        scale = Some(
                            v.parse().map_err(|_| CommandDecodeError::BadField("scale"))?,
                        )
                    }
                    "generation" => {
                        generation = Some(
                            v.parse().map_err(|_| CommandDecodeError::BadField("generation"))?,
                        )
                    }
                    "slot_offset" => {
                        slot_offset = Some(
                            v.parse().map_err(|_| CommandDecodeError::BadField("slot_offset"))?,
                        )
                    }
                    "col" => col = Some(v.parse().map_err(|_| CommandDecodeError::BadField("col"))?),
                    "row" => row = Some(v.parse().map_err(|_| CommandDecodeError::BadField("row"))?),
                    _ => {}
                }
            }
            Ok(Some(Command::RenderTile {
                correlation_id: cid,
                page: page.ok_or(CommandDecodeError::BadField("page"))?,
                x: x.ok_or(CommandDecodeError::BadField("x"))?,
                y: y.ok_or(CommandDecodeError::BadField("y"))?,
                w: w.ok_or(CommandDecodeError::BadField("w"))?,
                h: h.ok_or(CommandDecodeError::BadField("h"))?,
                scale: scale.ok_or(CommandDecodeError::BadField("scale"))?,
                generation: generation.ok_or(CommandDecodeError::BadField("generation"))?,
                slot_offset: slot_offset.ok_or(CommandDecodeError::BadField("slot_offset"))?,
                col: col.unwrap_or(0),
                row: row.unwrap_or(0),
            }))
        }
        "INSPECT" => Ok(Some(Command::Inspect {
            correlation_id: correlation_id.unwrap_or(0),
        })),
        "EXTRACT_PAGE" => {
            let cid = correlation_id.unwrap_or(0);
            let mut page_index = None;
            for line in text.lines().skip(1) {
                if let Some((k, v)) = line.split_once('=') {
                    if k == "page_index" {
                        page_index = Some(v.parse().map_err(|_| CommandDecodeError::BadField("page_index"))?);
                    }
                }
            }
            Ok(Some(Command::ExtractPage {
                correlation_id: cid,
                page_index: page_index.ok_or(CommandDecodeError::BadField("page_index"))?,
            }))
        }
        "GET_OUTLINE" => Ok(Some(Command::GetOutline {
            correlation_id: correlation_id.unwrap_or(0),
        })),
        "GET_LAYERS" => Ok(Some(Command::GetLayers {
            correlation_id: correlation_id.unwrap_or(0),
        })),
        "GET_ATTACHMENTS" => Ok(Some(Command::GetAttachments {
            correlation_id: correlation_id.unwrap_or(0),
        })),
        "GET_OBJECT" => {
            let cid = correlation_id.unwrap_or(0);
            let mut obj_num = None;
            for line in text.lines().skip(1) {
                if let Some((k, v)) = line.split_once('=') {
                    if k == "obj_num" {
                        obj_num = Some(v.parse().map_err(|_| CommandDecodeError::BadField("obj_num"))?);
                    }
                }
            }
            Ok(Some(Command::GetObject {
                correlation_id: cid,
                obj_num: obj_num.ok_or(CommandDecodeError::BadField("obj_num"))?,
            }))
        },
        "DELETE_PAGES" => {
            let cid = correlation_id.unwrap_or(0);
            let mut page_indices = None;
            for line in text.lines().skip(1) {
                if let Some((k, v)) = line.split_once('=') {
                    if k == "page_indices" {
                        let indices: Vec<u32> = v.split(',')
                            .filter(|s| !s.is_empty())
                            .map(|s| s.parse().map_err(|_| CommandDecodeError::BadField("page_indices")))
                            .collect::<Result<_, _>>()?;
                        page_indices = Some(indices);
                    }
                }
            }
            Ok(Some(Command::DeletePages {
                correlation_id: cid,
                page_indices: page_indices.ok_or(CommandDecodeError::BadField("page_indices"))?,
            }))
        }
        "ROTATE_PAGES" => {
            let cid = correlation_id.unwrap_or(0);
            let mut page_indices = None;
            let mut degrees = None;
            for line in text.lines().skip(1) {
                if let Some((k, v)) = line.split_once('=') {
                    match k {
                        "page_indices" => {
                            let indices: Vec<u32> = v.split(',')
                                .filter(|s| !s.is_empty())
                                .map(|s| s.parse().map_err(|_| CommandDecodeError::BadField("page_indices")))
                                .collect::<Result<_, _>>()?;
                            page_indices = Some(indices);
                        }
                        "degrees" => {
                            degrees = Some(v.parse().map_err(|_| CommandDecodeError::BadField("degrees"))?);
                        }
                        _ => {}
                    }
                }
            }
            Ok(Some(Command::RotatePages {
                correlation_id: cid,
                page_indices: page_indices.ok_or(CommandDecodeError::BadField("page_indices"))?,
                degrees: degrees.ok_or(CommandDecodeError::BadField("degrees"))?,
            }))
        }
        "ADD_ANNOTATION" => {
            let cid = correlation_id.unwrap_or(0);
            let mut page_index = None;
            let mut annotation_type = None;
            let mut rect = None;
            let mut contents = None;
            let mut color = None;
            for line in text.lines().skip(1) {
                if let Some((k, v)) = line.split_once('=') {
                    match k {
                        "page_index" => page_index = Some(v.parse().map_err(|_| CommandDecodeError::BadField("page_index"))?),
                        "annotation_type" => annotation_type = Some(v.to_string()),
                        "rect" => rect = Some(v.to_string()),
                        "contents" => contents = Some(v.to_string()),
                        "color" => color = Some(v.to_string()),
                        _ => {}
                    }
                }
            }
            Ok(Some(Command::AddAnnotation {
                correlation_id: cid,
                page_index: page_index.ok_or(CommandDecodeError::BadField("page_index"))?,
                annotation_type: annotation_type.ok_or(CommandDecodeError::BadField("annotation_type"))?,
                rect: rect.ok_or(CommandDecodeError::BadField("rect"))?,
                contents,
                color,
            }))
        }
        "DELETE_ANNOTATION" => {
            let cid = correlation_id.unwrap_or(0);
            let mut page_index = None;
            let mut annotation_id = None;
            for line in text.lines().skip(1) {
                if let Some((k, v)) = line.split_once('=') {
                    match k {
                        "page_index" => page_index = Some(v.parse().map_err(|_| CommandDecodeError::BadField("page_index"))?),
                        "annotation_id" => annotation_id = Some(v.parse().map_err(|_| CommandDecodeError::BadField("annotation_id"))?),
                        _ => {}
                    }
                }
            }
            Ok(Some(Command::DeleteAnnotation {
                correlation_id: cid,
                page_index: page_index.ok_or(CommandDecodeError::BadField("page_index"))?,
                annotation_id: annotation_id.ok_or(CommandDecodeError::BadField("annotation_id"))?,
            }))
        }
        "QUIT" => Ok(Some(Command::Quit)),
        _ => Err(CommandDecodeError::UnknownCommand),
    }
}

// ---------------------------------------------------------------------------
// Legacy encode/decode (preserved for backward compatibility)
// ---------------------------------------------------------------------------

/// A render-tile request sent from coordinator to worker. [ADR-007, SDS §6]
#[derive(Debug, Clone, PartialEq)]
pub struct RenderTileRequest {
    /// 0-based page index.
    pub page: u32,
    /// Device-space X origin.
    pub x: u32,
    /// Device-space Y origin.
    pub y: u32,
    /// Width in device pixels.
    pub w: u32,
    /// Height in device pixels.
    pub h: u32,
    /// Scale factor.
    pub scale: f32,
    /// Render generation for invalidation.
    pub generation: u64,
    /// Byte offset into the shared-memory region where output goes.
    pub slot_offset: u32,
}

/// Encode a `RenderTileRequest` as a text frame body (legacy v1 format).
pub fn encode_render_tile(req: &RenderTileRequest) -> Vec<u8> {
    format!(
        "render_tile\nv1\npage={}\nx={}\ny={}\nw={}\nh={}\nscale={}\ngeneration={}\nslot_offset={}\n",
        req.page, req.x, req.y, req.w, req.h, req.scale, req.generation, req.slot_offset,
    )
    .into_bytes()
}

/// Decode a `RenderTileRequest` from a text frame body (legacy v1 format).
pub fn decode_render_tile(body: &[u8]) -> Result<RenderTileRequest, CommandDecodeError> {
    let text = std::str::from_utf8(body).map_err(|_| CommandDecodeError::InvalidUtf8)?;
    let mut lines = text.lines();
    if lines.next() != Some("render_tile") {
        return Err(CommandDecodeError::UnknownCommand);
    }
    if lines.next() != Some("v1") {
        return Err(CommandDecodeError::UnknownCommand);
    }
    let mut page = None;
    let mut x = None;
    let mut y = None;
    let mut w = None;
    let mut h = None;
    let mut scale = None;
    let mut generation = None;
    let mut slot_offset = None;
    for line in lines {
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        match k {
            "page" => page = Some(v.parse().map_err(|_| CommandDecodeError::BadField("page"))?),
            "x" => x = Some(v.parse().map_err(|_| CommandDecodeError::BadField("x"))?),
            "y" => y = Some(v.parse().map_err(|_| CommandDecodeError::BadField("y"))?),
            "w" => w = Some(v.parse().map_err(|_| CommandDecodeError::BadField("w"))?),
            "h" => h = Some(v.parse().map_err(|_| CommandDecodeError::BadField("h"))?),
            "scale" => {
                scale = Some(v.parse().map_err(|_| CommandDecodeError::BadField("scale"))?)
            }
            "generation" => {
                generation =
                    Some(v.parse().map_err(|_| CommandDecodeError::BadField("generation"))?)
            }
            "slot_offset" => {
                slot_offset =
                    Some(v.parse().map_err(|_| CommandDecodeError::BadField("slot_offset"))?)
            }
            _ => {}
        }
    }
    Ok(RenderTileRequest {
        page: page.ok_or(CommandDecodeError::BadField("page"))?,
        x: x.ok_or(CommandDecodeError::BadField("x"))?,
        y: y.ok_or(CommandDecodeError::BadField("y"))?,
        w: w.ok_or(CommandDecodeError::BadField("w"))?,
        h: h.ok_or(CommandDecodeError::BadField("h"))?,
        scale: scale.ok_or(CommandDecodeError::BadField("scale"))?,
        generation: generation.ok_or(CommandDecodeError::BadField("generation"))?,
        slot_offset: slot_offset.ok_or(CommandDecodeError::BadField("slot_offset"))?,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_render_tile_roundtrip() {
        let cmd = Command::RenderTile {
            correlation_id: 42,
            page: 3,
            x: 256,
            y: 512,
            w: 128,
            h: 64,
            scale: 2.0,
            generation: 7,
            slot_offset: 262_144,
            col: 1,
            row: 2,
        };
        let bytes = encode_command(&cmd);
        let decoded = decode_command(&bytes).unwrap();
        assert_eq!(cmd, decoded);
    }

    #[test]
    fn typed_inspect_roundtrip() {
        let cmd = Command::Inspect { correlation_id: 99 };
        let bytes = encode_command(&cmd);
        let decoded = decode_command(&bytes).unwrap();
        assert_eq!(cmd, decoded);
    }

    #[test]
    fn typed_quit_roundtrip() {
        let cmd = Command::Quit;
        let bytes = encode_command(&cmd);
        let decoded = decode_command(&bytes).unwrap();
        assert_eq!(cmd, decoded);
    }

    #[test]
    fn legacy_render_tile_decodes_as_typed() {
        let req = RenderTileRequest {
            page: 0,
            x: 0,
            y: 0,
            w: 256,
            h: 256,
            scale: 1.5,
            generation: 42,
            slot_offset: 0,
        };
        let bytes = encode_render_tile(&req);
        let cmd = decode_command(&bytes).unwrap();
        // Legacy decode wraps with correlation_id=0.
        assert_eq!(cmd.correlation_id(), Some(0));
        match cmd {
            Command::RenderTile { page, w, h, scale, generation, .. } => {
                assert_eq!(page, 0);
                assert_eq!(w, 256);
                assert_eq!(h, 256);
                assert!((scale - 1.5).abs() < f32::EPSILON);
                assert_eq!(generation, 42);
            }
            other => panic!("expected RenderTile, got {other:?}"),
        }
    }

    #[test]
    fn legacy_inspect_decodes() {
        let cmd = decode_command(b"inspect").unwrap();
        assert_eq!(cmd, Command::Inspect { correlation_id: 0 });
    }

    #[test]
    fn legacy_quit_decodes() {
        let cmd = decode_command(b"quit").unwrap();
        assert_eq!(cmd, Command::Quit);
    }

    #[test]
    fn render_tile_codec_roundtrip() {
        let req = RenderTileRequest {
            page: 0,
            x: 0,
            y: 0,
            w: 256,
            h: 256,
            scale: 1.5,
            generation: 42,
            slot_offset: 0,
        };
        let bytes = encode_render_tile(&req);
        let decoded = decode_render_tile(&bytes).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn render_tile_with_offset() {
        let req = RenderTileRequest {
            page: 3,
            x: 256,
            y: 512,
            w: 128,
            h: 64,
            scale: 2.0,
            generation: 7,
            slot_offset: 262_144,
        };
        let bytes = encode_render_tile(&req);
        let decoded = decode_render_tile(&bytes).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn unknown_command_returns_error() {
        assert!(matches!(
            decode_command(b"bogus"),
            Err(CommandDecodeError::UnknownCommand)
        ));
    }

    #[test]
    fn typed_delete_pages_roundtrip() {
        let cmd = Command::DeletePages {
            correlation_id: 55,
            page_indices: vec![0, 2, 4],
        };
        let bytes = encode_command(&cmd);
        let decoded = decode_command(&bytes).unwrap();
        assert_eq!(cmd, decoded);
    }

    #[test]
    fn typed_rotate_pages_roundtrip() {
        let cmd = Command::RotatePages {
            correlation_id: 66,
            page_indices: vec![1, 3],
            degrees: 90,
        };
        let bytes = encode_command(&cmd);
        let decoded = decode_command(&bytes).unwrap();
        assert_eq!(cmd, decoded);
    }

    #[test]
    fn delete_pages_empty_indices() {
        let cmd = Command::DeletePages {
            correlation_id: 1,
            page_indices: vec![],
        };
        let bytes = encode_command(&cmd);
        let decoded = decode_command(&bytes).unwrap();
        assert_eq!(cmd, decoded);
    }

    #[test]
    fn typed_add_annotation_roundtrip() {
        let cmd = Command::AddAnnotation {
            correlation_id: 77,
            page_index: 2,
            annotation_type: "highlight".into(),
            rect: "100,200,50,12".into(),
            contents: None,
            color: Some("1.0,1.0,0.0,0.5".into()),
        };
        let bytes = encode_command(&cmd);
        let decoded = decode_command(&bytes).unwrap();
        assert_eq!(cmd, decoded);
    }

    #[test]
    fn typed_add_annotation_with_contents() {
        let cmd = Command::AddAnnotation {
            correlation_id: 78,
            page_index: 0,
            annotation_type: "sticky_note".into(),
            rect: "50,50,20,20".into(),
            contents: Some("TODO: review this".into()),
            color: None,
        };
        let bytes = encode_command(&cmd);
        let decoded = decode_command(&bytes).unwrap();
        assert_eq!(cmd, decoded);
    }

    #[test]
    fn typed_delete_annotation_roundtrip() {
        let cmd = Command::DeleteAnnotation {
            correlation_id: 88,
            page_index: 1,
            annotation_id: 42,
        };
        let bytes = encode_command(&cmd);
        let decoded = decode_command(&bytes).unwrap();
        assert_eq!(cmd, decoded);
    }
}
