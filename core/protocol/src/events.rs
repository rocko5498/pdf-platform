//! Events: worker → coordinator wire types + coordinator → shell in-process types. [ADR-004, SDS §5]
//!
//! Wire format: `EVT:<type>:<correlation_id>\n<key>=<value>\n...`
//! Mirrors the command envelope so both directions share one codec style.

use std::fmt;

use crate::commands::CorrelationId;
use crate::handles::TileSlotDesc;
use crate::inspect::StructuralSummary;

// ---------------------------------------------------------------------------
// Worker → coordinator events (wire types)
// ---------------------------------------------------------------------------

/// An event sent from a worker back to the coordinator. [SDS §4.3, §5.1]
///
/// Every event carries the `correlation_id` of the command it responds to,
/// enabling the coordinator to route the result to the correct caller.
#[derive(Debug, Clone, PartialEq)]
pub enum WorkerEvent {
    /// A tile has been rendered into shared memory. [ADR-007, SDS §6.4]
    TileReady {
        /// Correlation ID from the originating `RenderTile` command.
        correlation_id: CorrelationId,
        /// Descriptor of the rendered tile in shared memory.
        desc: TileSlotDesc,
    },
    /// A render or processing error occurred. [GR-8, SDS §5.5]
    RenderError {
        /// Correlation ID from the originating command.
        correlation_id: CorrelationId,
        /// Human-readable error description.
        message: String,
    },
    /// Structural summary from an inspect command. [SDS §3.1]
    Summary {
        /// Correlation ID from the originating `Inspect` command.
        correlation_id: CorrelationId,
        /// The scanned document structure.
        summary: StructuralSummary,
    },
    /// Text extraction result for a page. [ADR-019, M2]
    TextExtracted {
        /// Correlation ID from the originating `ExtractPage` command.
        correlation_id: CorrelationId,
        /// 0-based page index.
        page_index: u32,
        /// Number of text lines extracted.
        line_count: u32,
        /// Total character count.
        char_count: u32,
        /// Whether the text layer is reliable.
        reliable: bool,
        /// Whether the page has a tagged structure tree.
        has_structure: bool,
        /// Full page text (lines joined by newlines). Escaped on the wire.
        full_text: String,
        /// Per-line geometry: each entry is "idx|x|y|w|h|text" with text escaped
        /// (newlines as \\n, pipes as \\p). [ADR-019, FR-SRCH]
        line_geom: Vec<String>,
    },
    /// Document outline (bookmarks) result. [FR-BOOK, M1]
    OutlineResult {
        /// Correlation ID from the originating `GetOutline` command.
        correlation_id: CorrelationId,
        /// Number of top-level entries.
        entry_count: u32,
        /// Total entries (recursive).
        total_count: u32,
        /// Serialized outline data (JSON-like for now).
        data: String,
    },
    /// Optional content groups (layers) result. [FR-LAYER, M1]
    LayersResult {
        /// Correlation ID from the originating `GetLayers` command.
        correlation_id: CorrelationId,
        /// Number of top-level layer groups.
        group_count: u32,
        /// Total layers (recursive).
        total_count: u32,
        /// Whether layers are present.
        has_layers: bool,
    },
    /// Embedded file attachments result. [FR-EMB, M1]
    AttachmentsResult {
        /// Correlation ID from the originating `GetAttachments` command.
        correlation_id: CorrelationId,
        /// Number of attachments.
        count: u32,
        /// Serialized attachment list.
        data: String,
    },
    /// Raw object bytes response. [SDS §3.1]
    ///
    /// Returned by `GetObject` commands. The coordinator uses this to read
    /// document structure (Pages /Kids, page /Rotate, AcroForm, etc.)
    /// that isn't in the structural summary.
    ObjectData {
        /// Correlation ID from the originating `GetObject` command.
        correlation_id: CorrelationId,
        /// 1-based object number.
        obj_num: u32,
        /// Raw bytes of the object (including "N 0 obj\n...\nendobj\n").
        data: Vec<u8>,
    },
}

// ---------------------------------------------------------------------------
// Wire codec
// ---------------------------------------------------------------------------

/// Encode a [`WorkerEvent`] as a text frame body.
pub fn encode_worker_event(event: &WorkerEvent) -> Vec<u8> {
    match event {
        WorkerEvent::TileReady { correlation_id, desc } => {
            format!(
                "EVT:TILE_READY:{correlation_id}\n\
                 offset={}\nlen={}\nformat={}\ngeneration={}\n\
                 page={}\ncol={}\nrow={}\n",
                desc.offset,
                desc.len,
                desc.format as u32,
                desc.generation,
                desc.page,
                desc.col,
                desc.row,
            )
            .into_bytes()
        }
        WorkerEvent::RenderError {
            correlation_id,
            message,
        } => {
            // Escape newlines in the message to keep the wire format line-based.
            let escaped = message.replace('\n', "\\n");
            format!("EVT:RENDER_ERROR:{correlation_id}\nmessage={escaped}\n").into_bytes()
        }
        WorkerEvent::Summary {
            correlation_id,
            summary,
        } => {
            let mut out = format!(
                "EVT:SUMMARY:{correlation_id}\n\
                 page_count={}\nhas_acroform={}\nhas_xfa={}\nhas_js={}\n\
                 sig_count={}\nleniency_count={}\n",
                summary.page_count,
                u8::from(summary.has_acroform),
                u8::from(summary.has_xfa),
                u8::from(summary.has_js),
                summary.sig_count,
                summary.leniency_count,
            );
            // Leniency events are newline-separated, flattened with a count prefix.
            for event in &summary.leniency_events {
                out.push_str(&format!("leniency_event={event}\n"));
            }
            // Page dimensions as bit-patterns for f32 Eq compatibility.
            for (w_bits, h_bits, r) in &summary.page_dimensions {
                out.push_str(&format!("page_dim={},{},{}\n",
                    f32::from_bits(*w_bits), f32::from_bits(*h_bits), r));
            }
            // Xref offsets for incremental save.
            let mut sorted_offsets: Vec<_> = summary.original_offsets.iter().collect();
            sorted_offsets.sort_by_key(|(k, _)| **k);
            for (obj_num, offset) in sorted_offsets {
                out.push_str(&format!("xref_off={obj_num}:{offset}\n"));
            }
            out.into_bytes()
        }
        WorkerEvent::TextExtracted {
            correlation_id,
            page_index,
            line_count,
            char_count,
            reliable,
            has_structure,
            full_text,
            line_geom,
        } => {
            let mut out = format!(
                "EVT:TEXT_EXTRACTED:{correlation_id}\n\
page_index={page_index}\nline_count={line_count}\n\
char_count={char_count}\nreliable={reliable}\n\
has_structure={has_structure}\nfull_text={}\n",
                full_text.replace('\n', "\\n").replace('\r', "\\r")
            );
            for g in line_geom {
                out.push_str("line_geom=");
                out.push_str(g);
                out.push('\n');
            }
            out.into_bytes()
        }
        WorkerEvent::OutlineResult { correlation_id, entry_count, total_count, data } => {
            format!(
                "EVT:OUTLINE_RESULT:{correlation_id}\n\
                 entry_count={entry_count}\ntotal_count={total_count}\n\
                 data={}\n",
                data.replace('\n', "\\n")
            )
            .into_bytes()
        }
        WorkerEvent::LayersResult { correlation_id, group_count, total_count, has_layers } => {
            format!(
                "EVT:LAYERS_RESULT:{correlation_id}\n\
                 group_count={group_count}\ntotal_count={total_count}\n\
                 has_layers={has_layers}\n"
            )
            .into_bytes()
        }
        WorkerEvent::AttachmentsResult { correlation_id, count, data } => {
            format!(
                "EVT:ATTACHMENTS_RESULT:{correlation_id}\n\
                 count={count}\ndata={}\n",
                data.replace('\n', "\\n")
            )
            .into_bytes()
        }
        WorkerEvent::ObjectData { correlation_id, obj_num, data } => {
            // Hex-encode the raw bytes for safe text transport.
            let hex: String = data.iter().map(|b| format!("{b:02x}")).collect();
            format!(
                "EVT:OBJECT_DATA:{correlation_id}\n\
                 obj_num={obj_num}\ndata_len={}\ndata={hex}\n",
                data.len()
            )
            .into_bytes()
        }
    }
}

/// Errors from decoding a worker event frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventDecodeError {
    /// Not valid UTF-8.
    InvalidUtf8,
    /// Missing or unknown event type header.
    UnknownEvent,
    /// Missing or invalid field.
    BadField(&'static str),
}

impl fmt::Display for EventDecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUtf8 => write!(f, "event not utf-8"),
            Self::UnknownEvent => write!(f, "unknown event type"),
            Self::BadField(k) => write!(f, "bad field: {k}"),
        }
    }
}

impl std::error::Error for EventDecodeError {}

/// Decode a [`WorkerEvent`] from a text frame body.
pub fn decode_worker_event(body: &[u8]) -> Result<WorkerEvent, EventDecodeError> {
    let text = std::str::from_utf8(body).map_err(|_| EventDecodeError::InvalidUtf8)?;
    let mut lines = text.lines();

    // Parse "EVT:<TYPE>:<correlation_id>"
    let Some(first_line) = lines.next() else {
        return Err(EventDecodeError::UnknownEvent);
    };
    let Some(rest) = first_line.strip_prefix("EVT:") else {
        return Err(EventDecodeError::UnknownEvent);
    };
    let Some((type_str, id_str)) = rest.split_once(':') else {
        return Err(EventDecodeError::UnknownEvent);
    };
    let correlation_id: CorrelationId = id_str
        .parse()
        .map_err(|_| EventDecodeError::BadField("correlation_id"))?;

    match type_str {
        "TILE_READY" => {
            let mut offset = None;
            let mut len = None;
            let mut format = None;
            let mut generation = None;
            let mut page = None;
            let mut col = None;
            let mut row = None;
            for line in lines {
                let Some((k, v)) = line.split_once('=') else {
                    continue;
                };
                match k {
                    "offset" => {
                        offset = Some(v.parse().map_err(|_| EventDecodeError::BadField("offset"))?)
                    }
                    "len" => {
                        len = Some(v.parse().map_err(|_| EventDecodeError::BadField("len"))?)
                    }
                    "format" => {
                        format =
                            Some(v.parse().map_err(|_| EventDecodeError::BadField("format"))?)
                    }
                    "generation" => {
                        generation = Some(
                            v.parse().map_err(|_| EventDecodeError::BadField("generation"))?,
                        )
                    }
                    "page" => {
                        page = Some(v.parse().map_err(|_| EventDecodeError::BadField("page"))?)
                    }
                    "col" => {
                        col = Some(v.parse().map_err(|_| EventDecodeError::BadField("col"))?)
                    }
                    "row" => {
                        row = Some(v.parse().map_err(|_| EventDecodeError::BadField("row"))?)
                    }
                    _ => {}
                }
            }
            Ok(WorkerEvent::TileReady {
                correlation_id,
                desc: TileSlotDesc {
                    offset: offset.ok_or(EventDecodeError::BadField("offset"))?,
                    len: len.ok_or(EventDecodeError::BadField("len"))?,
                    format: crate::handles::PixelFormat::from_u32(
                        format.ok_or(EventDecodeError::BadField("format"))?,
                    )
                    .ok_or(EventDecodeError::BadField("format"))?,
                    generation: generation.ok_or(EventDecodeError::BadField("generation"))?,
                    page: page.ok_or(EventDecodeError::BadField("page"))?,
                    col: col.ok_or(EventDecodeError::BadField("col"))?,
                    row: row.ok_or(EventDecodeError::BadField("row"))?,
                },
            })
        }
        "RENDER_ERROR" => {
            let mut message = None;
            for line in lines {
                let Some((k, v)) = line.split_once('=') else {
                    continue;
                };
                if k == "message" {
                    // Un-escape newlines.
                    message = Some(v.replace("\\n", "\n"));
                }
            }
            Ok(WorkerEvent::RenderError {
                correlation_id,
                message: message.ok_or(EventDecodeError::BadField("message"))?,
            })
        }
        "SUMMARY" => {
            let mut page_count = None;
            let mut has_acroform = None;
            let mut has_xfa = None;
            let mut has_js = None;
            let mut sig_count = None;
            let mut leniency_count = None;
            let mut leniency_events = Vec::new();
            let mut page_dimensions = Vec::new();
            let mut original_offsets = std::collections::HashMap::new();
            for line in lines {
                let Some((k, v)) = line.split_once('=') else {
                    continue;
                };
                match k {
                    "page_count" => {
                        page_count = Some(
                            v.parse()
                                .map_err(|_| EventDecodeError::BadField("page_count"))?,
                        )
                    }
                    "has_acroform" => {
                        has_acroform = Some(
                            v.parse::<u8>()
                                .map_err(|_| EventDecodeError::BadField("has_acroform"))?
                                != 0,
                        )
                    }
                    "has_xfa" => {
                        has_xfa = Some(
                            v.parse::<u8>()
                                .map_err(|_| EventDecodeError::BadField("has_xfa"))?
                                != 0,
                        )
                    }
                    "has_js" => {
                        has_js = Some(
                            v.parse::<u8>()
                                .map_err(|_| EventDecodeError::BadField("has_js"))?
                                != 0,
                        )
                    }
                    "sig_count" => {
                        sig_count = Some(
                            v.parse()
                                .map_err(|_| EventDecodeError::BadField("sig_count"))?,
                        )
                    }
                    "leniency_count" => {
                        leniency_count = Some(
                            v.parse()
                                .map_err(|_| EventDecodeError::BadField("leniency_count"))?,
                        )
                    }
                    "leniency_event" => {
                        leniency_events.push(v.to_string());
                    }
                    "page_dim" => {
                        let dims: Vec<&str> = v.split(',').collect();
                        if dims.len() == 3 {
                            if let (Ok(w), Ok(h), Ok(r)) = (
                                dims[0].parse::<f32>(),
                                dims[1].parse::<f32>(),
                                dims[2].parse::<u32>(),
                            ) {
                                page_dimensions.push((w.to_bits(), h.to_bits(), r));
                            }
                        }
                    }
                    "xref_off" => {
                        if let Some((obj_s, off_s)) = v.split_once(':') {
                            if let (Ok(obj), Ok(off)) = (obj_s.parse::<u32>(), off_s.parse::<u32>()) {
                                original_offsets.insert(obj, off);
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(WorkerEvent::Summary {
                correlation_id,
                summary: StructuralSummary {
                    page_count: page_count.ok_or(EventDecodeError::BadField("page_count"))?,
                    has_acroform: has_acroform.ok_or(EventDecodeError::BadField("has_acroform"))?,
                    has_xfa: has_xfa.ok_or(EventDecodeError::BadField("has_xfa"))?,
                    has_js: has_js.ok_or(EventDecodeError::BadField("has_js"))?,
                    sig_count: sig_count.ok_or(EventDecodeError::BadField("sig_count"))?,
                    leniency_count: leniency_count
                        .ok_or(EventDecodeError::BadField("leniency_count"))?,
                    leniency_events,
                    page_dimensions,
                    original_offsets,
                },
            })
        }
        "TEXT_EXTRACTED" => {
            let mut page_index = None;
            let mut line_count = None;
            let mut char_count = None;
            let mut reliable = None;
            let mut has_structure = None;
            let mut full_text = None;
            let mut line_geom = Vec::new();
            for line in lines {
                let Some((k, v)) = line.split_once('=') else {
                    continue;
                };
                match k {
                    "page_index" => {
                        page_index = Some(v.parse().map_err(|_| EventDecodeError::BadField("page_index"))?)
                    }
                    "line_count" => {
                        line_count = Some(v.parse().map_err(|_| EventDecodeError::BadField("line_count"))?)
                    }
                    "char_count" => {
                        char_count = Some(v.parse().map_err(|_| EventDecodeError::BadField("char_count"))?)
                    }
                    "reliable" => {
                        reliable = Some(v == "true");
                    }
                    "has_structure" => {
                        has_structure = Some(v == "true");
                    }
                    "full_text" => {
                        full_text = Some(v.replace("\\n", "\n").replace("\\r", "\r"));
                    }
                    "line_geom" => {
                        line_geom.push(v.to_string());
                    }
                    _ => {}
                }
            }
            Ok(WorkerEvent::TextExtracted {
                correlation_id,
                page_index: page_index.ok_or(EventDecodeError::BadField("page_index"))?,
                line_count: line_count.ok_or(EventDecodeError::BadField("line_count"))?,
                char_count: char_count.ok_or(EventDecodeError::BadField("char_count"))?,
                reliable: reliable.ok_or(EventDecodeError::BadField("reliable"))?,
                has_structure: has_structure.unwrap_or(false),
                full_text: full_text.unwrap_or_default(),
                line_geom,
            })
        }
        "OUTLINE_RESULT" => {
            let mut entry_count = None;
            let mut total_count = None;
            let mut data = None;
            for line in lines {
                let Some((k, v)) = line.split_once('=') else { continue; };
                match k {
                    "entry_count" => entry_count = Some(v.parse().map_err(|_| EventDecodeError::BadField("entry_count"))?),
                    "total_count" => total_count = Some(v.parse().map_err(|_| EventDecodeError::BadField("total_count"))?),
                    "data" => data = Some(v.replace("\\n", "\n")),
                    _ => {}
                }
            }
            Ok(WorkerEvent::OutlineResult {
                correlation_id,
                entry_count: entry_count.ok_or(EventDecodeError::BadField("entry_count"))?,
                total_count: total_count.ok_or(EventDecodeError::BadField("total_count"))?,
                data: data.unwrap_or_default(),
            })
        }
        "LAYERS_RESULT" => {
            let mut group_count = None;
            let mut total_count = None;
            let mut has_layers = None;
            for line in lines {
                let Some((k, v)) = line.split_once('=') else { continue; };
                match k {
                    "group_count" => group_count = Some(v.parse().map_err(|_| EventDecodeError::BadField("group_count"))?),
                    "total_count" => total_count = Some(v.parse().map_err(|_| EventDecodeError::BadField("total_count"))?),
                    "has_layers" => has_layers = Some(v == "true"),
                    _ => {}
                }
            }
            Ok(WorkerEvent::LayersResult {
                correlation_id,
                group_count: group_count.ok_or(EventDecodeError::BadField("group_count"))?,
                total_count: total_count.ok_or(EventDecodeError::BadField("total_count"))?,
                has_layers: has_layers.ok_or(EventDecodeError::BadField("has_layers"))?,
            })
        }
        "ATTACHMENTS_RESULT" => {
            let mut count = None;
            let mut data = None;
            for line in lines {
                let Some((k, v)) = line.split_once('=') else { continue; };
                match k {
                    "count" => count = Some(v.parse().map_err(|_| EventDecodeError::BadField("count"))?),
                    "data" => data = Some(v.replace("\\n", "\n")),
                    _ => {}
                }
            }
            Ok(WorkerEvent::AttachmentsResult {
                correlation_id,
                count: count.ok_or(EventDecodeError::BadField("count"))?,
                data: data.unwrap_or_default(),
            })
        }
        "OBJECT_DATA" => {
            let mut obj_num = None;
            let mut data_len = None;
            let mut data_hex = None;
            for line in lines {
                let Some((k, v)) = line.split_once('=') else { continue; };
                match k {
                    "obj_num" => obj_num = Some(v.parse().map_err(|_| EventDecodeError::BadField("obj_num"))?),
                    "data_len" => data_len = Some(v.parse::<usize>().map_err(|_| EventDecodeError::BadField("data_len"))?),
                    "data" => data_hex = Some(v.to_string()),
                    _ => {}
                }
            }
            let hex = data_hex.ok_or(EventDecodeError::BadField("data"))?;
            let mut data = Vec::with_capacity(data_len.unwrap_or(hex.len() / 2));
            let mut chars = hex.chars();
            while let (Some(h1), Some(h2)) = (chars.next(), chars.next()) {
                let byte_str = format!("{h1}{h2}");
                data.push(u8::from_str_radix(&byte_str, 16)
                    .map_err(|_| EventDecodeError::BadField("data"))?);
            }
            Ok(WorkerEvent::ObjectData {
                correlation_id,
                obj_num: obj_num.ok_or(EventDecodeError::BadField("obj_num"))?,
                data,
            })
        }
        _ => Err(EventDecodeError::UnknownEvent),
    }
}

// ---------------------------------------------------------------------------
// Coordinator → shell events (in-process types)
// ---------------------------------------------------------------------------

/// Why a document worker is considered dead. [SDS §10.1]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerDeathReason {
    /// IPC channel closed / broken (peer gone).
    IpcDisconnected,
    /// OS process exited (`Child::try_wait`).
    ProcessExited {
        /// Exit code when available (None if signalled / unknown on platform).
        code: Option<i32>,
    },
    /// I/O or other failure treated as death (honest failure, not silent alive).
    Io {
        /// Display message only (no raw `io::Error` across FFI yet).
        message: String,
    },
}

/// Events produced by the coordinator for clients (shell, CLI, tests).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoordinatorEvent {
    /// Worker for a session is gone. [SDS §10.1 detection]
    WorkerDied {
        /// Local session id (not durable across app restarts).
        session_id: u64,
        /// How death was observed.
        reason: WorkerDeathReason,
    },
    /// Document has been saved. [ADR-012]
    DocumentSaved {
        /// Path the document was saved to.
        path: String,
        /// Revision number after save.
        revision: u64,
    },
    /// Undo/redo availability changed. [ADR-013]
    UndoStateChanged {
        /// Whether undo is available.
        can_undo: bool,
        /// Whether redo is available.
        can_redo: bool,
        /// Name of the next undoable action (if any).
        undo_name: Option<String>,
        /// Name of the next redoable action (if any).
        redo_name: Option<String>,
    },
    /// Document is dirty (has unsaved changes).
    DirtyStateChanged {
        /// Whether the document has unsaved changes.
        dirty: bool,
    },
}

// ---------------------------------------------------------------------------
// Legacy TILE_READY codec (preserved for backward compatibility)
// ---------------------------------------------------------------------------

/// Legacy encode for `TileSlotDesc` (v2 format, used by existing worker code).
pub fn encode_tile_ready_legacy(desc: &TileSlotDesc) -> Vec<u8> {
    crate::handles::encode_tile_ready(desc)
}

/// Legacy decode for `TileSlotDesc` (accepts v1 and v2).
pub fn decode_tile_ready_legacy(body: &[u8]) -> Result<TileSlotDesc, crate::handles::TileReadyDecodeError> {
    crate::handles::decode_tile_ready(body)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handles::{PixelFormat, TileSlotDesc};

    #[test]
    fn tile_ready_roundtrip() {
        let event = WorkerEvent::TileReady {
            correlation_id: 42,
            desc: TileSlotDesc {
                offset: 0,
                len: 262_144,
                format: PixelFormat::Rgba8,
                generation: 7,
                page: 3,
                col: 1,
                row: 2,
            },
        };
        let bytes = encode_worker_event(&event);
        let decoded = decode_worker_event(&bytes).unwrap();
        assert_eq!(event, decoded);
    }

    #[test]
    fn render_error_roundtrip() {
        let event = WorkerEvent::RenderError {
            correlation_id: 5,
            message: "page 99999 out of range".to_string(),
        };
        let bytes = encode_worker_event(&event);
        let decoded = decode_worker_event(&bytes).unwrap();
        assert_eq!(event, decoded);
    }

    #[test]
    fn render_error_escapes_newlines() {
        let event = WorkerEvent::RenderError {
            correlation_id: 1,
            message: "line1\nline2\nline3".to_string(),
        };
        let bytes = encode_worker_event(&event);
        let text = std::str::from_utf8(&bytes).unwrap();
        // Newlines in the message should be escaped.
        assert!(!text.contains("line1\nline2"));
        let decoded = decode_worker_event(&bytes).unwrap();
        assert_eq!(decoded, event);
    }

    #[test]
    fn summary_roundtrip() {
        let mut offsets = std::collections::HashMap::new();
        offsets.insert(1, 100);
        offsets.insert(3, 250);
        let event = WorkerEvent::Summary {
            correlation_id: 99,
            summary: StructuralSummary {
                page_count: 42,
                has_acroform: true,
                has_xfa: false,
                has_js: true,
                sig_count: 1,
                leniency_count: 2,
                leniency_events: vec!["repaired xref".into(), "missing font".into()],
                page_dimensions: vec![(612.0f32.to_bits(), 792.0f32.to_bits(), 0)],
                original_offsets: offsets,
            },
        };
        let bytes = encode_worker_event(&event);
        let decoded = decode_worker_event(&bytes).unwrap();
        assert_eq!(event, decoded);
    }

    #[test]
    fn unknown_event_returns_error() {
        assert!(matches!(
            decode_worker_event(b"EVT:BOGUS:0\n"),
            Err(EventDecodeError::UnknownEvent)
        ));
    }

    #[test]
    fn object_data_roundtrip() {
        let event = WorkerEvent::ObjectData {
            correlation_id: 42,
            obj_num: 3,
            data: b"3 0 obj\n<< /Type /Page /Parent 2 0 R >>\nendobj\n".to_vec(),
        };
        let bytes = encode_worker_event(&event);
        let decoded = decode_worker_event(&bytes).unwrap();
        assert_eq!(event, decoded);
    }

    #[test]
    fn object_data_empty() {
        let event = WorkerEvent::ObjectData {
            correlation_id: 1,
            obj_num: 99,
            data: vec![],
        };
        let bytes = encode_worker_event(&event);
        let decoded = decode_worker_event(&bytes).unwrap();
        assert_eq!(event, decoded);
    }
}
