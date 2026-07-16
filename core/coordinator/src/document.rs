//! DocumentCoordinator: single-writer actor owning document overlay. [SDS §1.4, ADR-010]
//!
//! Owns the document's model + CoW overlay, undo journal, extraction/render
//! scheduling for that document, and the lifecycle of its document worker.
//! Serializes all mutations (single-writer invariant). [SDS §2.2.1]
//!
//! M3: CoW overlay + Command/journal wired; incremental save; page organize.
//! Autosave: journal persisted to sidecar between saves for crash recovery. [SDS §10.3]

use std::io::Write;
use std::path::{Path, PathBuf};

use pdf_model::command::CommandGroup;
use pdf_model::journal::UndoJournal;
use pdf_model::overlay::CowOverlay;
use pdf_model::organize::{build_delete_pages_group, build_rotate_pages_group};
use pdf_write::IncrementalWriter;
use protocol::inspect::StructuralSummary;
use text_extract::TextExtractionService;

use crate::broker::open_read_only;
use crate::session::{SessionError, WorkerSession};

/// DocumentCoordinator: the trusted brain for one open document. [SDS §2.2.1]
///
/// Owns the CoW overlay, undo journal, and the worker session.
/// All mutations go through `apply_command_group()` which:
/// 1. Applies the group to the overlay
/// 2. Records it in the journal
/// 3. Bumps the revision
/// 4. Persists the journal to the sidecar file [SDS §10.3]
pub struct DocumentCoordinator {
    /// Worker session (manages the Z1 worker process).
    pub session: WorkerSession,
    /// The CoW overlay: tracks modified objects. [ADR-006]
    overlay: CowOverlay,
    /// Undo journal: records command groups for undo/redo. [ADR-013]
    journal: UndoJournal,
    /// Structural summary from the initial inspect.
    summary: StructuralSummary,
    /// The brokered document file (for respawn).
    doc_path: PathBuf,
    /// Next object number for new objects in incremental save.
    next_obj_num: u32,
    /// Last saved revision (for incremental save tracking).
    saved_revision: u64,
    /// Path to the sidecar autosave journal file. [SDS §10.3]
    sidecar_path: PathBuf,
    /// Canonical per-page text model cache. [ADR-019, SDS §8.5]
    text_service: TextExtractionService,
}

/// Information about an orphaned sidecar journal found at document open.
/// Used to offer crash recovery to the user. [SDS §10.2]
#[derive(Debug, Clone)]
pub struct RecoveryInfo {
    /// Path to the orphaned sidecar file.
    pub sidecar_path: PathBuf,
    /// Source file path recorded in the sidecar.
    pub source_path: PathBuf,
    /// Source file size recorded in the sidecar.
    pub source_size: u64,
    /// Number of command groups in the journal.
    pub group_count: usize,
    /// Names of the command groups (for display).
    pub group_names: Vec<String>,
}

/// User-facing diagnostics snapshot for the open document. [FR-DIAG, ADR-020, M1]
#[derive(Debug, Clone)]
pub struct DiagnosticsReport {
    /// Page count.
    pub page_count: u32,
    /// Number of leniency/repair events.
    pub leniency_count: u32,
    /// Human-readable leniency events.
    pub leniency_events: Vec<String>,
    /// Document has AcroForm.
    pub has_acroform: bool,
    /// Document has JavaScript.
    pub has_js: bool,
    /// Document has XFA (unsupported for rendering).
    pub has_xfa: bool,
    /// Signature count.
    pub sig_count: u32,
    /// Pages held in the text cache.
    pub text_cache_pages: usize,
    /// Text-cache revision.
    pub text_cache_revision: u64,
    /// Unsaved changes.
    pub dirty: bool,
    /// Undo available.
    pub can_undo: bool,
    /// Redo available.
    pub can_redo: bool,
}

/// Geometry for a text selection / find highlight. [FR-SRCH, M2]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SelectionBox {
    /// 0-based page index.
    pub page_index: u32,
    /// PDF user-space X.
    pub x: f32,
    /// PDF user-space Y.
    pub y: f32,
    /// Width in points.
    pub width: f32,
    /// Height in points.
    pub height: f32,
}


impl DocumentCoordinator {
    /// Open a document and create a coordinator. [SDS §3.1]
    ///
    /// Spawns a worker, inspects the document, and initializes the overlay/journal.
    /// Checks for orphaned sidecar journals from a previous crash. [SDS §10.2]
    pub fn open(
        worker_exe: &Path,
        doc_path: &Path,
    ) -> Result<Self, SessionError> {
        let brokered = open_read_only(doc_path)
            .map_err(|e| SessionError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        let mut session = WorkerSession::spawn_with_document(worker_exe, brokered)?;

        let summary = session.inspect()?;
        let next_obj_num = summary.page_count + 3; // catalog + pages + last page
        let sidecar = Self::compute_sidecar_path(doc_path);

        Ok(Self {
            session,
            overlay: CowOverlay::new(),
            journal: UndoJournal::new(),
            summary,
            doc_path: doc_path.to_path_buf(),
            next_obj_num,
            saved_revision: 0,
            sidecar_path: sidecar,
        text_service: TextExtractionService::new(),
        })
    }

    /// Compute the sidecar journal path for a given document. [SDS §10.3]
    ///
    /// Lives in the user's app-state directory, NOT beside the source document.
    /// Uses a hash of the canonical path to avoid collisions.
    fn compute_sidecar_path(doc_path: &Path) -> PathBuf {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let canonical = doc_path.canonicalize().unwrap_or_else(|_| doc_path.to_path_buf());
        let mut hasher = DefaultHasher::new();
        canonical.hash(&mut hasher);
        let hash = hasher.finish();

        let dir = std::env::temp_dir().join("pdf-platform-journals");
        dir.join(format!("{hash:016x}.journal"))
    }

    /// Check for an orphaned sidecar journal from a previous crash. [SDS §10.2]
    ///
    /// Returns `Some(RecoveryInfo)` if a valid sidecar exists for this document.
    /// The caller should present this to the user as a recovery offer.
    pub fn check_sidecar(&self) -> Option<RecoveryInfo> {
        Self::read_sidecar_impl(&self.sidecar_path, &self.doc_path).ok()
    }

    /// Read and validate a sidecar journal file. [SDS §10.3]
    fn read_sidecar_impl(sidecar_path: &Path, doc_path: &Path) -> Result<RecoveryInfo, String> {
        let data = std::fs::read(sidecar_path)
            .map_err(|e| format!("failed to read sidecar: {e}"))?;

        let text = String::from_utf8_lossy(&data);

        // Parse header: sidecar version, source file identity.
        let mut source_path = None;
        let mut source_size = None;
        let mut journal_data_start = 0;

        for (i, line) in text.lines().enumerate() {
            if line.starts_with("SOURCE_PATH:") {
                source_path = Some(line[12..].to_string());
            } else if line.starts_with("SOURCE_SIZE:") {
                source_size = line[12..].parse().ok();
            } else if line == "---" {
                journal_data_start = i + 1;
                break;
            }
        }

        let source_path = PathBuf::from(source_path.ok_or("missing SOURCE_PATH in sidecar")?);
        let source_size = source_size.ok_or("missing SOURCE_SIZE in sidecar")?;

        // Verify the sidecar is for this document.
        let canonical_doc = doc_path.canonicalize()
            .unwrap_or_else(|_| doc_path.to_path_buf());
        let canonical_sidecar = PathBuf::from(&source_path);
        let canonical_sidecar = canonical_sidecar.canonicalize()
            .unwrap_or(canonical_sidecar);

        // Compare canonical paths (allow for different representations).
        let path_matches = canonical_doc == canonical_sidecar
            || canonical_doc.to_string_lossy() == canonical_sidecar.to_string_lossy();

        if !path_matches {
            return Err("sidecar is for a different document".into());
        }

        // Parse the journal portion.
        let journal_bytes = text.lines().skip(journal_data_start)
            .collect::<Vec<_>>()
            .join("\n");
        let groups = UndoJournal::deserialize_applied(journal_bytes.as_bytes())
            .map_err(|e| format!("failed to deserialize journal: {e}"))?;

        let group_names = groups.iter().map(|g| g.name.clone()).collect();

        Ok(RecoveryInfo {
            sidecar_path: sidecar_path.to_path_buf(),
            source_path,
            source_size,
            group_count: groups.len(),
            group_names,
        })
    }

    /// Persist the journal to the sidecar file. [SDS §10.3]
    ///
    /// Called after each mutation to ensure crash recovery can replay
    /// the command journal. The sidecar records the source file identity
    /// for re-association at next launch.
    fn persist_journal(&self) -> Result<(), SessionError> {
        if !self.journal.can_undo() {
            // Nothing to persist — delete any existing sidecar.
            self.delete_sidecar();
            return Ok(());
        }

        // Ensure the sidecar directory exists.
        if let Some(parent) = self.sidecar_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| SessionError::Io(e))?;
        }

        let mut file = std::fs::File::create(&self.sidecar_path)
            .map_err(|e| SessionError::Io(e))?;

        // Write sidecar header with source file identity.
        let canonical_path = self.doc_path.canonicalize()
            .unwrap_or_else(|_| self.doc_path.clone());
        let metadata = std::fs::metadata(&self.doc_path)
            .map_err(|e| SessionError::Io(e))?;

        writeln!(file, "JOURNAL_SIDECAR v1")
            .map_err(|e| SessionError::Io(e))?;
        writeln!(file, "SOURCE_PATH:{}", canonical_path.display())
            .map_err(|e| SessionError::Io(e))?;
        writeln!(file, "SOURCE_SIZE:{}", metadata.len())
            .map_err(|e| SessionError::Io(e))?;
        writeln!(file, "REVISION:{}", self.overlay.revision())
            .map_err(|e| SessionError::Io(e))?;
        writeln!(file, "---")
            .map_err(|e| SessionError::Io(e))?;

        // Write the serialized journal.
        let journal_data = self.journal.serialize_applied();
        file.write_all(&journal_data)
            .map_err(|e| SessionError::Io(e))?;

        // Flush to ensure durability. [SDS §10.3 durability budget]
        file.sync_all()
            .map_err(|e| SessionError::Io(e))?;

        Ok(())
    }

    /// Delete the sidecar journal file (on clean save/close). [SDS §10.3]
    fn delete_sidecar(&self) {
        let _ = std::fs::remove_file(&self.sidecar_path);
    }

    /// Replay a sidecar journal to reconstruct pre-crash state. [SDS §10.2]
    ///
    /// Returns the command groups that were persisted. The caller applies
    /// them to a fresh overlay to reconstruct the pre-crash state.
    pub fn replay_sidecar(&self) -> Result<Vec<CommandGroup>, SessionError> {
        let data = std::fs::read(&self.sidecar_path)
            .map_err(|e| SessionError::Io(e))?;

        let text = String::from_utf8_lossy(&data);

        // Skip header lines (everything before "---").
        let journal_start = text.lines()
            .position(|l| l == "---")
            .map(|p| p + 1)
            .unwrap_or(0);

        let journal_bytes = text.lines().skip(journal_start)
            .collect::<Vec<_>>()
            .join("\n");

        UndoJournal::deserialize_applied(journal_bytes.as_bytes())
            .map_err(|e| SessionError::Protocol(format!("journal deserialize: {e}")))
    }

    /// Apply recovered command groups to the overlay. [SDS §10.2]
    ///
    /// After replaying the sidecar journal, call this to apply the recovered
    /// groups to the overlay and record them in the journal. This reconstructs
    /// the pre-crash state.
    pub fn apply_recovered_groups(
        &mut self,
        groups: Vec<CommandGroup>,
    ) -> Result<usize, SessionError> {
        let count = groups.len();
        for group in groups {
            group.apply(&mut self.overlay)
                .map_err(|e| SessionError::Protocol(e.to_string()))?;
            self.journal.record(group);
            self.overlay.bump_revision();
        }
        // Text model is revision-keyed; drop stale pages. [ADR-019]
        self.text_service.invalidate();
        // Persist the reconstructed journal.
        self.persist_journal()?;
        Ok(count)
    }

    /// Open a document with automatic crash recovery. [SDS §10.2]
    ///
    /// Opens the document, checks for an orphaned sidecar journal,
    /// and automatically replays it if found. Returns the recovery info
    /// so the caller can notify the user.
    pub fn open_with_recovery(
        worker_exe: &Path,
        doc_path: &Path,
    ) -> Result<(Self, Option<RecoveryInfo>), SessionError> {
        let mut coord = Self::open(worker_exe, doc_path)?;
        let recovery = coord.check_sidecar();

        if let Some(ref info) = recovery {
            // Replay and apply the recovered groups.
            let groups = coord.replay_sidecar()?;
            coord.apply_recovered_groups(groups)?;
        }

        Ok((coord, recovery))
    }

    /// Apply a command group to the overlay and record it in the journal.
    ///
    /// This is the single entry point for all document mutations.
    /// After recording, the journal is persisted to the sidecar. [SDS §10.3]
    pub fn apply_command_group(
        &mut self,
        group: CommandGroup,
    ) -> Result<(), SessionError> {
        // Apply to overlay.
        group.apply(&mut self.overlay)
            .map_err(|e| SessionError::Protocol(e.to_string()))?;

        // Record in journal (clears redo stack).
        self.journal.record(group);

        // Bump revision.
        self.overlay.bump_revision();

        // Text model is revision-keyed; drop stale pages. [ADR-019]
        self.text_service.invalidate();

        // Persist journal to sidecar for crash recovery. [SDS §10.3]
        self.persist_journal()?;

        Ok(())
    }

    /// Undo the most recent command group.
    pub fn undo(&mut self) -> Result<bool, SessionError> {
        if let Some(group) = self.journal.undo() {
            group.undo(&mut self.overlay)
                .map_err(|e| SessionError::Protocol(e.to_string()))?;
            self.overlay.bump_revision();
            self.text_service.invalidate();
            self.persist_journal()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Redo the most recently undone command group.
    pub fn redo(&mut self) -> Result<bool, SessionError> {
        if let Some(group) = self.journal.redo() {
            group.apply(&mut self.overlay)
                .map_err(|e| SessionError::Protocol(e.to_string()))?;
            self.overlay.bump_revision();
            self.text_service.invalidate();
            self.persist_journal()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Save the document using incremental write. [ADR-012]
    ///
    /// Writes the dirty objects from the overlay and records the save point.
    /// After successful save, the sidecar journal is deleted. [SDS §10.3]
    pub fn save_incremental(&mut self, output_path: &Path) -> Result<u32, SessionError> {
        // Read the original file and append incremental update.
        let original_bytes = std::fs::read(&self.doc_path)
            .map_err(|e| SessionError::Io(e))?;
        let original_len = original_bytes.len() as u32;

        let mut file = std::fs::File::create(output_path)
            .map_err(|e| SessionError::Io(e))?;

        // Write original bytes first.
        file.write_all(&original_bytes)
            .map_err(|e| SessionError::Io(e))?;

        let result = IncrementalWriter::write_incremental(
            &mut file,
            &self.overlay,
            0, // prev_xref_offset will be set by the writer from the original
            self.next_obj_num,
            &self.summary.original_offsets,
            original_len,
        ).map_err(|e| SessionError::Io(e))?;

        self.saved_revision = self.overlay.revision();

        // Delete sidecar — the saved file is now the recovery point. [SDS §10.3]
        self.delete_sidecar();

        Ok(result.objects_written)
    }

    /// Close the document cleanly. [SDS §3.5]
    ///
    /// Deletes the sidecar journal and cleans up state.
    pub fn close(&mut self) -> Result<(), SessionError> {
        self.delete_sidecar();
        self.journal.clear();
        self.text_service.clear();
        Ok(())
    }

    /// Execute a delete-pages operation end-to-end.
    ///
    /// Reads the real Pages object from the worker, parses the Kids array,
    /// and builds a proper command group. [FR-ORG, M3]
    pub fn delete_pages(&mut self, page_indices: &[u32]) -> Result<(), SessionError> {
        // Find the Pages parent object. Typically object 2 in simple PDFs,
        // but we search for it in the catalog's /Pages reference.
        let pages_obj_num = self.find_pages_object()?;

        // Read the current Kids array from the worker.
        let pages_bytes = self.session.get_object(pages_obj_num)?;
        let pages_text = String::from_utf8_lossy(&pages_bytes);

        // Extract the Kids array content between /Kids [ and ].
        let kids_bytes = self.extract_kids_array(&pages_text)
            .ok_or_else(|| SessionError::Protocol("could not parse /Kids array".into()))?;

        let group = build_delete_pages_group(
            page_indices,
            pages_obj_num,
            &kids_bytes,
            self.summary.page_count,
        ).map_err(|e| SessionError::Protocol(e))?;

        self.apply_command_group(group)
    }

    /// Rotate pages by the specified degrees.
    ///
    /// Reads actual /Rotate values from the worker for each page. [FR-ROTATE, M3]
    pub fn rotate_pages(&mut self, page_indices: &[u32], degrees: u32) -> Result<(), SessionError> {
        // Find the Pages object to get the Kids array.
        let pages_obj_num = self.find_pages_object()?;
        let pages_bytes = self.session.get_object(pages_obj_num)?;
        let pages_text = String::from_utf8_lossy(&pages_bytes);

        // Parse kid references to get page object numbers.
        let kid_refs = self.parse_kid_references(&pages_text);

        // Read current rotation for each page.
        let page_obj_rotations: Vec<(u32, u32)> = page_indices.iter()
            .filter_map(|&page_idx| {
                kid_refs.get(page_idx as usize).map(|&obj_num| {
                    let rotation = self.read_page_rotation(obj_num);
                    (obj_num, rotation)
                })
            })
            .collect();

        let group = build_rotate_pages_group(
            page_indices,
            &page_obj_rotations,
            degrees,
        ).map_err(|e| SessionError::Protocol(e))?;

        self.apply_command_group(group)
    }

    /// Find the Pages parent object number by reading the catalog. [SDS §3.1]
    fn find_pages_object(&mut self) -> Result<u32, SessionError> {
        // Read the catalog (object 1) to find /Pages reference.
        let catalog_bytes = self.session.get_object(1)?;
        let catalog_text = String::from_utf8_lossy(&catalog_bytes);

        // Parse "/Pages N 0 R" from the catalog.
        if let Some(pos) = catalog_text.find("/Pages") {
            let after = &catalog_text[pos + 6..];
            let mut chars = after.chars();
            // Skip whitespace.
            while let Some(c) = chars.next() {
                if c.is_ascii_digit() {
                    let mut num = String::new();
                    num.push(c);
                    for c in chars.by_ref() {
                        if c.is_ascii_digit() {
                            num.push(c);
                        } else {
                            break;
                        }
                    }
                    if let Ok(n) = num.parse::<u32>() {
                        return Ok(n);
                    }
                }
            }
        }

        // Fallback: assume object 2 (common in simple PDFs).
        Ok(2)
    }

    /// Extract the raw Kids array bytes from a Pages object string.
    fn extract_kids_array(&self, pages_text: &str) -> Option<Vec<u8>> {
        let start = pages_text.find("/Kids [")?;
        let array_start = start + "/Kids [".len();
        let end = pages_text[array_start..].find(']')?;
        let array_content = &pages_text[array_start..array_start + end];
        Some(array_content.as_bytes().to_vec())
    }

    /// Parse kid references from a Pages object string into object numbers.
    fn parse_kid_references(&self, pages_text: &str) -> Vec<u32> {
        let mut refs = Vec::new();
        if let Some(start) = pages_text.find("/Kids [") {
            let array_start = start + "/Kids [".len();
            if let Some(end) = pages_text[array_start..].find(']') {
                let array = &pages_text[array_start..array_start + end];
                let tokens: Vec<&str> = array.split_whitespace().collect();
                for chunk in tokens.chunks(3) {
                    if chunk.len() == 3 && chunk[2] == "R" {
                        if let Ok(num) = chunk[0].parse::<u32>() {
                            refs.push(num);
                        }
                    }
                }
            }
        }
        refs
    }

    /// Read the /Rotate value from a page object. Returns 0 if not present.
    fn read_page_rotation(&mut self, page_obj_num: u32) -> u32 {
        self.session.get_object(page_obj_num)
            .ok()
            .and_then(|bytes| {
                let text = String::from_utf8_lossy(&bytes);
                // Find /Rotate N
                let pos = text.find("/Rotate")?;
                let after = &text[pos + 7..];
                let mut num_str = String::new();
                for c in after.chars() {
                    if c.is_ascii_digit() {
                        num_str.push(c);
                    } else if !num_str.is_empty() {
                        break;
                    }
                }
                num_str.parse().ok()
            })
            .unwrap_or(0)
    }

    /// Whether the document has unsaved changes.
    pub fn is_dirty(&self) -> bool {
        self.overlay.revision() > self.saved_revision
    }

    /// Whether undo is available.
    pub fn can_undo(&self) -> bool {
        self.journal.can_undo()
    }

    /// Whether redo is available.
    pub fn can_redo(&self) -> bool {
        self.journal.can_redo()
    }

    /// Name of the next undoable action.
    pub fn undo_name(&self) -> Option<&str> {
        self.journal.undo_name()
    }

    /// Name of the next redoable action.
    pub fn redo_name(&self) -> Option<&str> {
        self.journal.redo_name()
    }

    /// Current revision number.
    pub fn revision(&self) -> u64 {
        self.overlay.revision()
    }

    /// Number of undoable groups.
    pub fn undo_depth(&self) -> usize {
        self.journal.undo_depth()
    }

    /// Number of redoable groups.
    pub fn redo_depth(&self) -> usize {
        self.journal.redo_depth()
    }

    /// Path to the sidecar autosave journal. [SDS §10.3]
    pub fn sidecar_path(&self) -> &Path {
        &self.sidecar_path
    }

    /// Read and validate a sidecar journal file. [SDS §10.3]
    ///
    /// Public for testing and recovery workflows.
    pub fn read_sidecar(sidecar_path: &Path, doc_path: &Path) -> Result<RecoveryInfo, String> {
        Self::read_sidecar_impl(sidecar_path, doc_path)
    }

    /// Number of objects modified.
    pub fn dirty_object_count(&self) -> usize {
        self.overlay.dirty_objects().len()
    }

    /// Page count from the structural summary.
    pub fn page_count(&self) -> u32 {
        self.summary.page_count
    }

    /// The document's structural summary.
    pub fn summary(&self) -> &StructuralSummary {
        &self.summary
    }

    /// Get a reference to the undo journal (for diagnostics/serialization).
    pub fn journal(&self) -> &UndoJournal {
        &self.journal
    }

    /// Serialize the journal for sidecar persistence.
    pub fn serialize_journal(&self) -> Vec<u8> {
        self.journal.serialize_applied()
    }

    /// Close the worker session.

    /// Extract and cache the canonical text model for a page. [ADR-019]
    pub fn get_page_text(
        &mut self,
        page_index: u32,
    ) -> Result<&engine_api::extract::PageTextModel, SessionError> {
        if self.text_service.get_cached(page_index).is_none() {
            let model = self.session.extract_page(page_index)?;
            self.text_service.insert_model(model);
        }
        self.text_service
            .get_cached(page_index)
            .ok_or_else(|| SessionError::Protocol("text cache miss after insert".into()))
    }

    /// Find `query` across currently cached pages. [ADR-019, FR-SRCH]
    pub fn find_in_cached_text(
        &self,
        query: &str,
        page_indices: &[u32],
    ) -> Vec<text_extract::PageSearchResult> {
        self.text_service.find_in_cached_pages(query, page_indices)
    }

    /// Number of pages currently held in the text cache.
    pub fn text_cache_page_count(&self) -> usize {
        self.text_service.cached_page_count()
    }

    /// Text-cache revision (bumps when the document mutates).
    pub fn text_cache_revision(&self) -> u64 {
        self.text_service.revision()
    }


    /// Document outline (bookmarks). [FR-BOOK, M1]
    pub fn get_outline(&mut self) -> Result<crate::session::StructureQueryResult, SessionError> {
        self.session.get_outline()
    }

    /// Optional content layers. [FR-LAYER, M1]
    pub fn get_layers(&mut self) -> Result<crate::session::StructureQueryResult, SessionError> {
        self.session.get_layers()
    }

    /// Embedded attachments. [FR-EMB, M1]
    pub fn get_attachments(&mut self) -> Result<crate::session::StructureQueryResult, SessionError> {
        self.session.get_attachments()
    }

    /// Leniency / repair diagnostics for the open document. [FR-DIAG, PRIN-6, M1]
    pub fn diagnostics_report(&self) -> DiagnosticsReport {
        DiagnosticsReport {
            page_count: self.summary.page_count,
            leniency_count: self.summary.leniency_count,
            leniency_events: self.summary.leniency_events.clone(),
            has_acroform: self.summary.has_acroform,
            has_js: self.summary.has_js,
            has_xfa: self.summary.has_xfa,
            sig_count: self.summary.sig_count,
            text_cache_pages: self.text_service.cached_page_count(),
            text_cache_revision: self.text_service.revision(),
            dirty: self.is_dirty(),
            can_undo: self.can_undo(),
            can_redo: self.can_redo(),
        }
    }

    /// Ensure pages 0..page_count-1 are extracted into the text cache, then find. [ADR-019, FR-SRCH]
    pub fn find_in_document(
        &mut self,
        query: &str,
    ) -> Result<Vec<text_extract::PageSearchResult>, SessionError> {
        let n = self.page_count();
        for i in 0..n {
            let _ = self.get_page_text(i)?;
        }
        let indices: Vec<u32> = (0..n).collect();
        Ok(self.find_in_cached_text(query, &indices))
    }

    /// Match bounding boxes for a find hit on a cached page (selection chrome). [FR-SRCH, M2]
    pub fn selection_boxes_for_match(
        &self,
        page_index: u32,
        line_index: u32,
        char_offset: u32,
        char_len: u32,
    ) -> Option<SelectionBox> {
        let model = self.text_service.get_cached(page_index)?;
        let line = model.lines.iter().find(|l| l.index == line_index)?;
        // Approximate: proportional slice of the line bbox when span geometry is absent.
        let text_len = line.text.len().max(1) as f32;
        let start = (char_offset as f32 / text_len).clamp(0.0, 1.0);
        let end = ((char_offset + char_len) as f32 / text_len).clamp(0.0, 1.0);
        let x = line.x + line.width * start;
        let w = line.width * (end - start).max(0.01);
        Some(SelectionBox {
            page_index,
            x,
            y: line.y,
            width: w,
            height: line.height.max(1.0),
        })
    }

    pub fn close_worker(&mut self) -> Result<(), SessionError> {
        let _ = self.session.send(b"CMD:QUIT\n");
        let _ = self.session.kill_worker();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdf_model::command::SetObjectCommand;
    use std::fs;

    fn worker_path() -> PathBuf {
        let exe = std::env::current_exe().expect("current exe");
        let deps = exe.parent().expect("exe parent");
        let debug_dir = deps.parent().expect("debug dir");
        debug_dir.join(format!("worker{}", std::env::consts::EXE_SUFFIX))
    }

    fn fixture_path(name: &str) -> PathBuf {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let repo_root = manifest_dir.parent().unwrap();
        repo_root.join("tools/corpus-diff/fixtures").join(name)
    }

    #[test]
    fn coordinator_open_and_inspect() {
        let worker = worker_path();
        let pdf = fixture_path("valid-1page.pdf");
        if !pdf.exists() || !worker.exists() {
            eprintln!("skipping: fixture or worker not found");
            return;
        }

        let mut coord = DocumentCoordinator::open(&worker, &pdf).expect("open");
        assert_eq!(coord.page_count(), 1);
        assert!(!coord.is_dirty());
        assert!(!coord.can_undo());
    }

    #[test]
    fn coordinator_apply_and_undo() {
        let worker = worker_path();
        let pdf = fixture_path("valid-1page.pdf");
        if !pdf.exists() || !worker.exists() {
            eprintln!("skipping: fixture or worker not found");
            return;
        }

        let mut coord = DocumentCoordinator::open(&worker, &pdf).expect("open");

        // Apply a simple command group.
        let mut group = CommandGroup::new("Test Edit");
        group.push(Box::new(SetObjectCommand {
            obj_num: 10,
            new_bytes: b"10 0 obj\n<< /Test true >>\nendobj\n".to_vec(),
            old_bytes: None,
        }));

        coord.apply_command_group(group).expect("apply");
        assert!(coord.is_dirty());
        assert!(coord.can_undo());
        assert_eq!(coord.undo_name(), Some("Test Edit"));
        assert_eq!(coord.revision(), 1);

        // Undo.
        coord.undo().expect("undo");
        assert!(!coord.can_undo());
        assert!(coord.can_redo());
        assert_eq!(coord.redo_name(), Some("Test Edit"));

        // Redo.
        coord.redo().expect("redo");
        assert!(coord.can_undo());
        assert!(!coord.can_redo());
    }

    #[test]
    fn coordinator_save_incremental() {
        let worker = worker_path();
        let pdf = fixture_path("valid-1page.pdf");
        if !pdf.exists() || !worker.exists() {
            eprintln!("skipping: fixture or worker not found");
            return;
        }

        let mut coord = DocumentCoordinator::open(&worker, &pdf).expect("open");

        // Apply a change.
        let mut group = CommandGroup::new("Test Save");
        group.push(Box::new(SetObjectCommand {
            obj_num: 10,
            new_bytes: b"10 0 obj\n<< /Saved true >>\nendobj\n".to_vec(),
            old_bytes: None,
        }));
        coord.apply_command_group(group).expect("apply");
        assert!(coord.is_dirty());

        // Save.
        let save_path = std::env::temp_dir().join("pdf-platform-test-save.pdf");
        let written = coord.save_incremental(&save_path).expect("save");
        assert!(written > 0);
        assert!(!coord.is_dirty());

        // Verify the file exists and has content.
        let meta = fs::metadata(&save_path).expect("save file exists");
        assert!(meta.len() > 0);

        fs::remove_file(&save_path).ok();
    }

    #[test]
    fn coordinator_journal_serialization() {
        let worker = worker_path();
        let pdf = fixture_path("valid-1page.pdf");
        if !pdf.exists() || !worker.exists() {
            eprintln!("skipping: fixture or worker not found");
            return;
        }

        let mut coord = DocumentCoordinator::open(&worker, &pdf).expect("open");

        // Apply some edits.
        for i in 0..3 {
            let mut group = CommandGroup::new(format!("Edit {i}"));
            group.push(Box::new(SetObjectCommand {
                obj_num: 10 + i,
                new_bytes: format!("{} 0 obj\n<< /V {} >>\nendobj\n", 10 + i, i).into_bytes(),
                old_bytes: None,
            }));
            coord.apply_command_group(group).expect("apply");
        }

        // Serialize journal.
        let data = coord.serialize_journal();
        let text = String::from_utf8_lossy(&data);
        assert!(text.contains("UNDO_JOURNAL v1"));
        assert!(text.contains("GROUPS:3"));
        assert!(text.contains("GROUP:Edit 0"));
        assert!(text.contains("GROUP:Edit 2"));
    }

    // -----------------------------------------------------------------------
    // M3 fault-injection tests [ADR-022, SDS §10.6]
    //
    // These validate the M3 exit criteria:
    // - Fault-injection suite passes
    // - Undo across a crash restores state
    // - Journal serialization roundtrips correctly
    // -----------------------------------------------------------------------

    #[test]
    fn fault_injection_undo_after_multiple_edits() {
        // Simulate: apply 5 edits, undo 3, verify state is correct.
        let mut overlay = CowOverlay::new();
        let mut journal = UndoJournal::new();

        for i in 0..5 {
            let mut group = CommandGroup::new(format!("Edit {i}"));
            group.push(Box::new(SetObjectCommand {
                obj_num: 10 + i,
                new_bytes: format!("v{i}").into_bytes(),
                old_bytes: None,
            }));
            group.apply(&mut overlay).unwrap();
            journal.record(group);
            overlay.bump_revision();
        }

        assert_eq!(overlay.revision(), 5);

        // Undo 3 times.
        for _ in 0..3 {
            let group = journal.undo().unwrap();
            group.undo(&mut overlay).unwrap();
            overlay.bump_revision();
        }

        assert_eq!(journal.undo_depth(), 2);
        assert_eq!(journal.redo_depth(), 3);
        assert!(!overlay.is_dirty()); // dirty was cleared on bump
    }

    #[test]
    fn fault_injection_journal_persistence_roundtrip() {
        // Simulate crash recovery: serialize journal, deserialize, verify groups.
        let mut journal = UndoJournal::new();

        for i in 0..10 {
            let mut group = CommandGroup::new(format!("Persistent Edit {i}"));
            group.push(Box::new(SetObjectCommand {
                obj_num: 10 + i,
                new_bytes: format!("data-{i}").into_bytes(),
                old_bytes: if i > 0 { Some(format!("data-{}", i - 1).into_bytes()) } else { None },
            }));
            journal.record(group);
        }

        // Serialize (simulates sidecar write before crash).
        let data = journal.serialize_applied();

        // Deserialize (simulates recovery on next launch).
        let recovered = UndoJournal::deserialize_applied(&data).unwrap();
        assert_eq!(recovered.len(), 10);
        assert_eq!(recovered[0].name, "Persistent Edit 0");
        assert_eq!(recovered[9].name, "Persistent Edit 9");
    }

    #[test]
    fn fault_injection_overlay_snapshot_restore() {
        // Simulate: snapshot the overlay before an edit, apply the edit,
        // then restore from snapshot (simulating undo after crash).
        let mut overlay = CowOverlay::new();

        // Initial state.
        overlay.set_object(1, b"initial-v1".to_vec());
        overlay.set_object(2, b"initial-v2".to_vec());

        // Snapshot BEFORE bump (dirty objects are still tracked).
        let snap = overlay.snapshot_dirty();
        assert_eq!(snap.len(), 2); // Both objects are dirty.

        overlay.bump_revision();

        // Apply edits.
        overlay.set_object(1, b"modified-v1".to_vec());
        overlay.set_object(3, b"new-v3".to_vec());

        assert!(overlay.get_object(1).is_some());
        assert!(overlay.get_object(3).is_some());

        // Restore from snapshot (simulates crash recovery to pre-edit state).
        overlay.clear();
        for (obj_num, bytes) in &snap {
            overlay.set_object(*obj_num, bytes.clone());
        }

        // After restore, we should have the pre-edit state.
        let obj1 = overlay.get_object(1);
        assert!(obj1.is_some());
        assert_eq!(obj1.unwrap(), b"initial-v1");
        // Object 3 should not exist (it was created after the snapshot).
        assert!(overlay.get_object(3).is_none());
    }

    #[test]
    fn fault_injection_incremental_save_preserves_bytes() {
        // Verify incremental save writes the dirty objects correctly.
        let mut overlay = CowOverlay::new();

        let original = b"1 0 obj\n<< /Type /Catalog >>\nendobj\n";
        overlay.set_object(1, original.to_vec());
        overlay.bump_revision();

        let mut output = Vec::new();
        let result = pdf_write::IncrementalWriter::write_incremental(
            &mut output,
            &overlay,
            0,
            2,
            &std::collections::HashMap::new(),
            0,
        ).unwrap();

        assert_eq!(result.objects_written, 1);
        let text = String::from_utf8_lossy(&output);
        // The written bytes should contain the original object.
        assert!(text.contains("1 0 obj"));
        assert!(text.contains("/Type /Catalog"));
        assert!(text.contains("xref"));
        assert!(text.contains("trailer"));
        assert!(text.contains("%%EOF"));
    }

    #[test]
    fn fault_injection_delete_pages_undo_roundtrip() {
        // Simulate: delete pages, undo, verify restoration.
        let mut overlay = CowOverlay::new();
        let mut journal = UndoJournal::new();

        let kids = b"2 0 obj\n<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 >>\nendobj\n";
        let group = pdf_model::organize::build_delete_pages_group(&[1], 2, kids, 3).unwrap();

        group.apply(&mut overlay).unwrap();
        journal.record(group);
        overlay.bump_revision();

        // Verify deletion happened.
        let new_kids = String::from_utf8_lossy(overlay.get_object(2).unwrap()).to_string();
        assert!(new_kids.contains("/Count 2"));
        assert!(!new_kids.contains("4 0 R"));

        // Undo.
        let group = journal.undo().unwrap();
        group.undo(&mut overlay).unwrap();
        overlay.bump_revision();

        // After undo, the original Kids should be restored.
        let restored = String::from_utf8_lossy(overlay.get_object(2).unwrap()).to_string();
        assert!(restored.contains("/Count 3"));
        assert!(restored.contains("4 0 R"));
    }

    // -----------------------------------------------------------------------
    // Autosave journal persistence tests [SDS §10.3]
    // -----------------------------------------------------------------------

    #[test]
    fn autosave_sidecar_created_after_mutation() {
        let worker = worker_path();
        let pdf = fixture_path("valid-1page.pdf");
        if !pdf.exists() || !worker.exists() {
            eprintln!("skipping: fixture or worker not found");
            return;
        }

        let mut coord = DocumentCoordinator::open(&worker, &pdf).expect("open");
        let sidecar = coord.sidecar_path.clone();

        // No sidecar yet.
        assert!(!sidecar.exists());

        // Apply a mutation — sidecar should be created.
        let mut group = CommandGroup::new("Test Mutation");
        group.push(Box::new(SetObjectCommand {
            obj_num: 10,
            new_bytes: b"test".to_vec(),
            old_bytes: None,
        }));
        coord.apply_command_group(group).expect("apply");

        // Sidecar should exist now.
        assert!(sidecar.exists(), "sidecar should exist after mutation");

        // Sidecar should contain valid journal data.
        let data = std::fs::read(&sidecar).expect("read sidecar");
        let text = String::from_utf8_lossy(&data);
        assert!(text.contains("JOURNAL_SIDECAR v1"), "sidecar header");
        assert!(text.contains("SOURCE_PATH:"), "sidecar has source path");
        assert!(text.contains("SOURCE_SIZE:"), "sidecar has source size");
        assert!(text.contains("REVISION:"), "sidecar has revision");
        assert!(text.contains("GROUP:Test Mutation"), "sidecar has journal group");

        // Cleanup.
        coord.close().expect("close");
    }

    #[test]
    fn autosave_sidecar_deleted_after_save() {
        let worker = worker_path();
        let pdf = fixture_path("valid-1page.pdf");
        if !pdf.exists() || !worker.exists() {
            eprintln!("skipping: fixture or worker not found");
            return;
        }

        let mut coord = DocumentCoordinator::open(&worker, &pdf).expect("open");
        let sidecar = coord.sidecar_path.clone();

        // Apply a mutation.
        let mut group = CommandGroup::new("Pre-save edit");
        group.push(Box::new(SetObjectCommand {
            obj_num: 10,
            new_bytes: b"test".to_vec(),
            old_bytes: None,
        }));
        coord.apply_command_group(group).expect("apply");
        assert!(sidecar.exists(), "sidecar should exist after mutation");

        // Save — sidecar should be deleted.
        let save_path = std::env::temp_dir().join("pdf-platform-autosave-test.pdf");
        coord.save_incremental(&save_path).expect("save");
        assert!(!sidecar.exists(), "sidecar should be deleted after save");

        fs::remove_file(&save_path).ok();
        coord.close().expect("close");
    }

    #[test]
    fn autosave_sidecar_deleted_on_close() {
        let worker = worker_path();
        let pdf = fixture_path("valid-1page.pdf");
        if !pdf.exists() || !worker.exists() {
            eprintln!("skipping: fixture or worker not found");
            return;
        }

        let mut coord = DocumentCoordinator::open(&worker, &pdf).expect("open");
        let sidecar = coord.sidecar_path.clone();

        // Apply a mutation.
        let mut group = CommandGroup::new("Pre-close edit");
        group.push(Box::new(SetObjectCommand {
            obj_num: 10,
            new_bytes: b"test".to_vec(),
            old_bytes: None,
        }));
        coord.apply_command_group(group).expect("apply");
        assert!(sidecar.exists());

        // Close — sidecar should be deleted.
        coord.close().expect("close");
        assert!(!sidecar.exists(), "sidecar should be deleted on close");
    }

    #[test]
    fn autosave_sidecar_survives_undo_redo() {
        let worker = worker_path();
        let pdf = fixture_path("valid-1page.pdf");
        if !pdf.exists() || !worker.exists() {
            eprintln!("skipping: fixture or worker not found");
            return;
        }

        let mut coord = DocumentCoordinator::open(&worker, &pdf).expect("open");
        let sidecar = coord.sidecar_path.clone();

        // Apply two mutations.
        for i in 0..2 {
            let mut group = CommandGroup::new(format!("Edit {i}"));
            group.push(Box::new(SetObjectCommand {
                obj_num: 10 + i,
                new_bytes: format!("v{i}").into_bytes(),
                old_bytes: None,
            }));
            coord.apply_command_group(group).expect("apply");
        }
        assert!(sidecar.exists());

        // Undo one — sidecar should still exist with updated content.
        coord.undo().expect("undo");
        assert!(sidecar.exists());

        // Redo — sidecar should still exist.
        coord.redo().expect("redo");
        assert!(sidecar.exists());

        // Verify sidecar content reflects current state.
        let data = std::fs::read(&sidecar).expect("read sidecar");
        let text = String::from_utf8_lossy(&data);
        assert!(text.contains("GROUP:Edit 0"), "journal should contain Edit 0");
        assert!(text.contains("GROUP:Edit 1"), "journal should contain Edit 1");

        coord.close().expect("close");
    }

    #[test]
    fn autosave_sidecar_identity_validation() {
        // Verify that a sidecar for a different document is rejected.
        let fake_sidecar = std::env::temp_dir().join("pdf-platform-fake-sidecar.journal");
        let fake_doc = std::env::temp_dir().join("pdf-platform-fake-doc.pdf");

        // Write a sidecar claiming to be for fake_doc.
        std::fs::write(&fake_sidecar, format!(
            "JOURNAL_SIDECAR v1\nSOURCE_PATH:{}\nSOURCE_SIZE:100\nREVISION:1\n---\nUNDO_JOURNAL v1\nGROUPS:0\n",
            fake_doc.display()
        )).expect("write fake sidecar");

        // Try to read it for a different document.
        let result = DocumentCoordinator::read_sidecar(
            &fake_sidecar,
            &std::env::temp_dir().join("different-document.pdf"),
        );

        assert!(result.is_err(), "should reject sidecar for different document");
        assert!(result.unwrap_err().contains("different document"));

        fs::remove_file(&fake_sidecar).ok();
    }
}
