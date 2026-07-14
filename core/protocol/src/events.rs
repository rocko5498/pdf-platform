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
            out.into_bytes()
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
                },
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
}
