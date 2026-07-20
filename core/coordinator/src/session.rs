//! Worker session lifecycle: spawn, poll, death, inspect, respawn. [SDS §10.1, §3.1, ADR-008]
//!
//! M0: multi-process inspect + kill → respawn with document re-inherit.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::broker::BrokeredFile;
use protocol::commands::{encode_command, Command};
use protocol::events::{decode_worker_event, CoordinatorEvent, WorkerDeathReason, WorkerEvent};
use protocol::inspect::StructuralSummary;
use protocol::transport::TransportError;
use sandbox::spawn::{spawn_worker, spawn_worker_with_file, WorkerChild};

static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);

/// Errors from session operations.
#[derive(Debug)]
pub enum SessionError {
    /// Underlying OS / spawn I/O.
    Io(io::Error),
    /// Operation invalid for current state (e.g. respawn while alive).
    InvalidState(
        /// Short static description of the invalid transition.
        &'static str,
    ),
    /// Protocol / codec failure (inspect reply).
    Protocol(String),
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionError::Io(e) => write!(f, "session io: {e}"),
            SessionError::InvalidState(s) => write!(f, "session invalid state: {s}"),
            SessionError::Protocol(s) => write!(f, "session protocol: {s}"),
        }
    }
}

impl std::error::Error for SessionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SessionError::Io(e) => Some(e),
            SessionError::InvalidState(_) | SessionError::Protocol(_) => None,
        }
    }
}

impl From<io::Error> for SessionError {
    fn from(e: io::Error) -> Self {
        SessionError::Io(e)
    }
}

enum LiveState {
    Alive {
        child: WorkerChild,
    },
    Dead {
        reason: WorkerDeathReason,
        /// True after `WorkerDied` has been emitted once for this death.
        emitted: bool,
    },
}

/// One worker-backed session (optionally owns a brokered document).
pub struct WorkerSession {
    id: u64,
    worker_exe: PathBuf,
    state: LiveState,
    /// Z0-owned document; re-inherited on each spawn/respawn. [SDS §10.1 step 2]
    doc: Option<BrokeredFile>,
    /// Optional open password re-applied on respawn. [FR-VIEW encrypt]
    password: Option<String>,
    /// Monotonic counter for correlation IDs.
    next_cid: std::sync::atomic::AtomicU64,
}

/// Result of a document structure query (outline / layers / attachments). [M1]
#[derive(Debug, Clone, PartialEq)]
pub struct StructureQueryResult {
    /// Query kind label.
    pub kind: String,
    /// Primary count (top-level entries / groups / files).
    pub count: u32,
    /// Total count (recursive where applicable).
    pub total: u32,
    /// Serialized detail payload.
    pub data: String,
    /// Presence flag (has layers / non-empty).
    pub flag: bool,
}

/// Result of a redact-by-term operation. [FR-RED-3, M7]
#[derive(Debug, Clone)]
pub struct RedactByTermResult {
    /// Whether verification passed.
    pub passed: bool,
    /// Number of regions redacted.
    pub regions_redacted: u32,
    /// Number of content items confirmed removed.
    pub items_removed: u32,
    /// The full verification report text.
    pub report: String,
    /// Any remaining risks (empty on success).
    pub risks: Vec<String>,
}

/// Result of a page raster for OCR. [FR-OCR, M9]
#[derive(Debug, Clone)]
pub struct PageRasterResult {
    /// 0-based page index.
    pub page_index: u32,
    /// Output width in pixels.
    pub width: u32,
    /// Output height in pixels.
    pub height: u32,
    /// RGBA8 pixel data.
    pub pixels: Vec<u8>,
}

/// Minimal base64 decoder.
fn base64_decode(input: &str) -> Vec<u8> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(input.trim_end_matches('='))
        .unwrap_or_default()
}

fn parse_line_geom(g: &str) -> Option<engine_api::extract::TextLine> {
    // idx|x|y|w|h|text
    let parts: Vec<&str> = g.splitn(6, '|').collect();
    if parts.len() < 6 {
        return None;
    }
    let index: u32 = parts[0].parse().ok()?;
    let x: f32 = parts[1].parse().ok()?;
    let y: f32 = parts[2].parse().ok()?;
    let width: f32 = parts[3].parse().ok()?;
    let height: f32 = parts[4].parse().ok()?;
    let text = parts[5]
        .replace("\\n", "\n")
        .replace("\\p", "|")
        .replace("\\\\", "\\");
    Some(engine_api::extract::TextLine {
        index,
        text,
        x,
        y,
        width,
        height,
        spans: vec![],
    })
}

impl WorkerSession {
    /// Spawn a worker and attach it to a new session (no document).
    pub fn spawn(worker_exe: &Path) -> Result<Self, SessionError> {
        let child = spawn_worker(worker_exe)?;
        Ok(Self {
            id: NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed),
            worker_exe: worker_exe.to_path_buf(),
            state: LiveState::Alive { child },
            doc: None,
            password: None,
            next_cid: AtomicU64::new(1),
        })
    }

    /// Spawn a worker with a brokered document via inherited FD/HANDLE. [SDS §3.1, GR-1]
    ///
    /// Takes ownership of `doc` so the session can re-inherit on respawn.
    pub fn spawn_with_document(
        worker_exe: &Path,
        doc: BrokeredFile,
    ) -> Result<Self, SessionError> {
        Self::spawn_with_document_password(worker_exe, doc, None)
    }

    /// Spawn with document and optional user password for encryption. [FR-VIEW encrypt]
    pub fn spawn_with_document_password(
        worker_exe: &Path,
        doc: BrokeredFile,
        password: Option<&str>,
    ) -> Result<Self, SessionError> {
        let child = spawn_worker_with_file(worker_exe, doc.file(), password, &[])?;
        Ok(Self {
            id: NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed),
            worker_exe: worker_exe.to_path_buf(),
            state: LiveState::Alive { child },
            doc: Some(doc),
            password: password.map(str::to_string),
            next_cid: AtomicU64::new(1),
        })
    }

    /// Request a structural summary from the worker (`inspect` command).
    pub fn inspect(&mut self) -> Result<StructuralSummary, SessionError> {
        let correlation_id = self.next_correlation_id();
        let cmd = Command::Inspect { correlation_id };
        let body = encode_command(&cmd);
        self.send(&body)?;
        let reply = self.recv_frame(Duration::from_secs(30))?;
        match decode_worker_event(&reply) {
            Ok(WorkerEvent::Summary { correlation_id: cid, summary }) if cid == correlation_id => {
                Ok(summary)
            }
            Ok(other) => Err(SessionError::Protocol(
                format!("unexpected event: {other:?}"),
            )),
            Err(e) => {
                // Fall back to legacy SUMMARY codec for backward compatibility.
                protocol::inspect::decode_summary(&reply)
                    .map_err(|_| SessionError::Protocol(format!("decode failed: {e}")))
            }
        }
    }

    /// Fetch raw bytes of a PDF object from the worker. [SDS §3.1]
    ///
    /// Used by the coordinator to read document structure (Pages /Kids,
    /// page /Rotate, AcroForm, etc.) that isn't in the structural summary.
    pub fn get_object(&mut self, obj_num: u32) -> Result<Vec<u8>, SessionError> {
        let correlation_id = self.next_correlation_id();
        let cmd = Command::GetObject { correlation_id, obj_num };
        let body = encode_command(&cmd);
        self.send(&body)?;
        let reply = self.recv_frame(Duration::from_secs(10))?;
        match decode_worker_event(&reply) {
            Ok(WorkerEvent::ObjectData { correlation_id: cid, obj_num: on, data })
                if cid == correlation_id && on == obj_num =>
            {
                Ok(data)
            }
            Ok(WorkerEvent::RenderError { correlation_id: cid, message })
                if cid == correlation_id =>
            {
                Err(SessionError::Protocol(message))
            }
            Ok(other) => Err(SessionError::Protocol(
                format!("unexpected event: {other:?}"),
            )),
            Err(e) => Err(SessionError::Protocol(format!("decode failed: {e}"))),
        }
    }

    /// Evaluate a forms JS subset expression in the Z1 worker. [ADR-017, FR-JS-*, M5]
    ///
    /// After field values change in a full forms product path, callers must
    /// regenerate widget appearance streams (same honesty rule as annotations:
    /// never leave appearance-less visible widgets). [FR-ANNOT-2 pattern, M5]
    pub fn forms_calc(
        &mut self,
        expression: &str,
        fields: &[(String, f64)],
        enabled: bool,
    ) -> Result<(bool, f64, String), SessionError> {
        let correlation_id = self.next_correlation_id();
        let cmd = Command::FormsCalc {
            correlation_id,
            expression: expression.to_string(),
            fields: fields.to_vec(),
            enabled,
        };
        let body = encode_command(&cmd);
        self.send(&body)?;
        let reply = self.recv_frame(Duration::from_secs(10))?;
        match decode_worker_event(&reply) {
            Ok(WorkerEvent::FormsCalcResult {
                correlation_id: cid,
                ok,
                value,
                message,
            }) if cid == correlation_id => Ok((ok, value, message)),
            Ok(WorkerEvent::RenderError {
                correlation_id: cid,
                message,
            }) if cid == correlation_id => Err(SessionError::Protocol(message)),
            Ok(other) => Err(SessionError::Protocol(format!(
                "unexpected event: {other:?}"
            ))),
            Err(e) => Err(SessionError::Protocol(format!("decode failed: {e}"))),
        }
    }

    /// Extract the canonical text model for a page via the Z1 worker. [ADR-019]
    pub fn extract_page(
        &mut self,
        page_index: u32,
    ) -> Result<engine_api::extract::PageTextModel, SessionError> {
        use engine_api::extract::{PageTextModel, TextLine};

        let correlation_id = self.next_correlation_id();
        let cmd = Command::ExtractPage {
            correlation_id,
            page_index,
        };
        let body = encode_command(&cmd);
        self.send(&body)?;
        let reply = self.recv_frame(Duration::from_secs(30))?;
        match decode_worker_event(&reply) {
            Ok(WorkerEvent::TextExtracted {
                correlation_id: cid,
                page_index: pi,
                line_count: _,
                char_count,
                reliable,
                has_structure,
                full_text,
                line_geom,
            }) if cid == correlation_id && pi == page_index =>
            {
                let lines = if !line_geom.is_empty() {
                    line_geom
                        .iter()
                        .filter_map(|g| parse_line_geom(g))
                        .collect()
                } else if full_text.is_empty() {
                    Vec::new()
                } else {
                    full_text
                        .lines()
                        .enumerate()
                        .map(|(i, line)| TextLine {
                            index: i as u32,
                            text: line.to_string(),
                            x: 0.0,
                            y: 0.0,
                            width: 0.0,
                            height: 0.0,
                            spans: vec![],
                        })
                        .collect()
                };
                Ok(PageTextModel {
                    page_index,
                    lines,
                    reliable,
                    char_count,
                    has_structure,
                })
            }
            Ok(WorkerEvent::RenderError {
                correlation_id: cid,
                message,
            }) if cid == correlation_id => Err(SessionError::Protocol(message)),
            Ok(other) => Err(SessionError::Protocol(format!(
                "unexpected event: {other:?}"
            ))),
            Err(e) => Err(SessionError::Protocol(format!("decode failed: {e}"))),
        }
    }

    /// Query document outline (bookmarks). [FR-BOOK, M1]
    pub fn get_outline(&mut self) -> Result<StructureQueryResult, SessionError> {
        let correlation_id = self.next_correlation_id();
        let cmd = Command::GetOutline { correlation_id };
        self.send(&encode_command(&cmd))?;
        let reply = self.recv_frame(Duration::from_secs(10))?;
        match decode_worker_event(&reply) {
            Ok(WorkerEvent::OutlineResult {
                correlation_id: cid,
                entry_count,
                total_count,
                data,
            }) if cid == correlation_id => Ok(StructureQueryResult {
                kind: "outline".into(),
                count: entry_count,
                total: total_count,
                data,
                flag: total_count > 0,
            }),
            Ok(WorkerEvent::RenderError {
                correlation_id: cid,
                message,
            }) if cid == correlation_id => Err(SessionError::Protocol(message)),
            Ok(other) => Err(SessionError::Protocol(format!("unexpected: {other:?}"))),
            Err(e) => Err(SessionError::Protocol(format!("decode: {e}"))),
        }
    }

    /// Query optional content groups (layers). [FR-LAYER, M1]
    pub fn get_layers(&mut self) -> Result<StructureQueryResult, SessionError> {
        let correlation_id = self.next_correlation_id();
        let cmd = Command::GetLayers { correlation_id };
        self.send(&encode_command(&cmd))?;
        let reply = self.recv_frame(Duration::from_secs(10))?;
        match decode_worker_event(&reply) {
            Ok(WorkerEvent::LayersResult {
                correlation_id: cid,
                group_count,
                total_count,
                has_layers,
            }) if cid == correlation_id => Ok(StructureQueryResult {
                kind: "layers".into(),
                count: group_count,
                total: total_count,
                data: String::new(),
                flag: has_layers,
            }),
            Ok(WorkerEvent::RenderError {
                correlation_id: cid,
                message,
            }) if cid == correlation_id => Err(SessionError::Protocol(message)),
            Ok(other) => Err(SessionError::Protocol(format!("unexpected: {other:?}"))),
            Err(e) => Err(SessionError::Protocol(format!("decode: {e}"))),
        }
    }

    /// Query embedded file attachments. [FR-EMB, M1]
    pub fn get_attachments(&mut self) -> Result<StructureQueryResult, SessionError> {
        let correlation_id = self.next_correlation_id();
        let cmd = Command::GetAttachments { correlation_id };
        self.send(&encode_command(&cmd))?;
        let reply = self.recv_frame(Duration::from_secs(10))?;
        match decode_worker_event(&reply) {
            Ok(WorkerEvent::AttachmentsResult {
                correlation_id: cid,
                count,
                data,
            }) if cid == correlation_id => Ok(StructureQueryResult {
                kind: "attachments".into(),
                count,
                total: count,
                data,
                flag: count > 0,
            }),
            Ok(WorkerEvent::RenderError {
                correlation_id: cid,
                message,
            }) if cid == correlation_id => Err(SessionError::Protocol(message)),
            Ok(other) => Err(SessionError::Protocol(format!("unexpected: {other:?}"))),
            Err(e) => Err(SessionError::Protocol(format!("decode: {e}"))),
        }
    }

    /// Render a full page as a raster for OCR processing. [FR-OCR, M9]
    ///
    /// Sends a `RenderPageForOcr` command to the worker, which renders
    /// the entire page at the specified DPI scale and returns the RGBA8
    /// pixel data directly.
    pub fn render_page_for_ocr(
        &mut self,
        page_index: u32,
        scale: f32,
    ) -> Result<PageRasterResult, SessionError> {
        let correlation_id = self.next_correlation_id();
        let cmd = Command::RenderPageForOcr {
            correlation_id,
            page_index,
            scale,
        };
        let body = encode_command(&cmd);
        self.send(&body)?;
        let reply = self.recv_frame(Duration::from_secs(60))?;
        match decode_worker_event(&reply) {
            Ok(WorkerEvent::PageRasterReady {
                correlation_id: cid,
                page_index: pi,
                width,
                height,
                pixels_b64,
            }) if cid == correlation_id && pi == page_index => {
                // Decode base64 pixels.
                let pixels = base64_decode(&pixels_b64);
                Ok(PageRasterResult {
                    page_index,
                    width,
                    height,
                    pixels,
                })
            }
            Ok(WorkerEvent::RenderError {
                correlation_id: cid,
                message,
            }) if cid == correlation_id => Err(SessionError::Protocol(message)),
            Ok(other) => Err(SessionError::Protocol(format!("unexpected: {other:?}"))),
            Err(e) => Err(SessionError::Protocol(format!("decode: {e}"))),
        }
    }

    /// Redact text by search term across pages. [FR-RED-5, FR-RED-6, M7]
    ///
    /// Sends a `RedactByTerm` command to the worker, which searches for
    /// the term, marks matching regions, applies content removal, scrubs
    /// metadata, removes overlapping annotations, and verifies the result.
    pub fn redact_by_term(
        &mut self,
        search_term: &str,
        case_sensitive: bool,
        whole_word: bool,
        page_filter: Option<Vec<u32>>,
    ) -> Result<RedactByTermResult, SessionError> {
        let correlation_id = self.next_correlation_id();
        let cmd = Command::RedactByTerm {
            correlation_id,
            search_term: search_term.to_string(),
            case_sensitive,
            whole_word,
            page_filter,
        };
        let body = encode_command(&cmd);
        self.send(&body)?;
        let reply = self.recv_frame(Duration::from_secs(60))?;
        match decode_worker_event(&reply) {
            Ok(WorkerEvent::RedactResult {
                correlation_id: cid,
                passed,
                regions_redacted,
                items_removed,
                report,
                risks,
            }) if cid == correlation_id => Ok(RedactByTermResult {
                passed,
                regions_redacted,
                items_removed,
                report,
                risks,
            }),
            Ok(WorkerEvent::RenderError {
                correlation_id: cid,
                message,
            }) if cid == correlation_id => Err(SessionError::Protocol(message)),
            Ok(other) => Err(SessionError::Protocol(format!("unexpected: {other:?}"))),
            Err(e) => Err(SessionError::Protocol(format!("decode: {e}"))),
        }
    }

    /// Local session id (stable across respawn).
    pub fn session_id(&self) -> u64 {
        self.id
    }

    /// Whether the worker is currently considered alive.
    pub fn is_alive(&self) -> bool {
        matches!(self.state, LiveState::Alive { .. })
    }

    /// Whether this session owns a brokered document.
    pub fn has_document(&self) -> bool {
        self.doc.is_some()
    }

    /// Generate the next unique correlation ID for command-response matching.
    pub fn next_correlation_id(&self) -> u64 {
        self.next_cid.fetch_add(1, Ordering::Relaxed)
    }

    /// Path used for (re)spawn of the worker binary.
    pub fn worker_exe(&self) -> &Path {
        &self.worker_exe
    }

    /// Send a frame to the live worker (tests / ping).
    pub fn send(&mut self, frame: &[u8]) -> Result<(), SessionError> {
        match &mut self.state {
            LiveState::Alive { child } => child
                .transport
                .send(frame)
                .map_err(transport_to_session_err),
            LiveState::Dead { .. } => Err(SessionError::InvalidState("worker dead")),
        }
    }

    /// Receive one frame with timeout (tests / inspect).
    pub fn recv_frame(&mut self, timeout: Duration) -> Result<Vec<u8>, SessionError> {
        match &mut self.state {
            LiveState::Alive { child } => child
                .transport
                .recv_timeout(timeout)
                .map_err(transport_to_session_err),
            LiveState::Dead { .. } => Err(SessionError::InvalidState("worker dead")),
        }
    }

    /// Fault-injection: kill the worker process. [ADR-022]
    pub fn kill_worker(&mut self) -> Result<(), SessionError> {
        match &mut self.state {
            LiveState::Alive { child } => {
                child.child.kill()?;
                let _ = child.child.wait();
                Ok(())
            }
            LiveState::Dead { .. } => Ok(()),
        }
    }

    /// Poll for liveness / inbound frames / death. [SDS §10.1]
    pub fn poll(&mut self, timeout: Duration) -> Result<Vec<CoordinatorEvent>, SessionError> {
        match &mut self.state {
            LiveState::Dead { reason, emitted } => {
                if *emitted {
                    return Ok(vec![]);
                }
                *emitted = true;
                let reason = reason.clone();
                Ok(vec![CoordinatorEvent::WorkerDied {
                    session_id: self.id,
                    reason,
                }])
            }
            LiveState::Alive { child } => {
                if let Some(status) = child.child.try_wait()? {
                    let reason = WorkerDeathReason::ProcessExited {
                        code: status.code(),
                    };
                    self.state = LiveState::Dead {
                        reason: reason.clone(),
                        emitted: true,
                    };
                    return Ok(vec![CoordinatorEvent::WorkerDied {
                        session_id: self.id,
                        reason,
                    }]);
                }

                match child.transport.recv_timeout(timeout) {
                    Ok(_frame) => Ok(vec![]),
                    Err(TransportError::Timeout) => Ok(vec![]),
                    Err(TransportError::Disconnected) => {
                        let reason = WorkerDeathReason::IpcDisconnected;
                        self.state = LiveState::Dead {
                            reason: reason.clone(),
                            emitted: true,
                        };
                        Ok(vec![CoordinatorEvent::WorkerDied {
                            session_id: self.id,
                            reason,
                        }])
                    }
                    Err(TransportError::FrameTooLarge { max, got }) => {
                        let reason = WorkerDeathReason::Io {
                            message: format!("frame too large: {got} (max {max})"),
                        };
                        self.state = LiveState::Dead {
                            reason: reason.clone(),
                            emitted: true,
                        };
                        Ok(vec![CoordinatorEvent::WorkerDied {
                            session_id: self.id,
                            reason,
                        }])
                    }
                    Err(TransportError::Io(e)) => {
                        let reason = WorkerDeathReason::Io {
                            message: e.to_string(),
                        };
                        self.state = LiveState::Dead {
                            reason: reason.clone(),
                            emitted: true,
                        };
                        Ok(vec![CoordinatorEvent::WorkerDied {
                            session_id: self.id,
                            reason,
                        }])
                    }
                }
            }
        }
    }

    /// After death: spawn a fresh worker; re-inherit document if present. [SDS §10.1]
    ///
    /// Session id is unchanged. Does not replay overlays (none in M0).
    pub fn respawn(&mut self) -> Result<(), SessionError> {
        match &self.state {
            LiveState::Alive { .. } => {
                return Err(SessionError::InvalidState("respawn while alive"));
            }
            LiveState::Dead { .. } => {}
        }
        let child = if let Some(doc) = self.doc.as_ref() {
            spawn_worker_with_file(&self.worker_exe, doc.file(), self.password.as_deref(), &[])?
        } else {
            spawn_worker(&self.worker_exe)?
        };
        self.state = LiveState::Alive { child };
        Ok(())
    }

    /// Alias for [`Self::respawn`] when no document is attached (ping-only).
    pub fn respawn_ping_only(&mut self) -> Result<(), SessionError> {
        if self.doc.is_some() {
            return Err(SessionError::InvalidState(
                "use respawn() when a document is attached",
            ));
        }
        self.respawn()
    }
}

fn transport_to_session_err(e: TransportError) -> SessionError {
    match e {
        TransportError::Io(ioe) => SessionError::Io(ioe),
        other => SessionError::Io(io::Error::new(io::ErrorKind::Other, other.to_string())),
    }
}
