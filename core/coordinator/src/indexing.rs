//! Cross-document index enrollment orchestration. [ADR-019 §3, ADR-034, SDS §2.2.9]
//!
//! Walks an enrolled root (bounds enforced by
//! [`crate::broker::IndexEnrollmentRegistry`]), extracts canonical text via
//! the normal document-open path (`DocumentCoordinator::get_page_text` —
//! already sandboxed, nothing new), stages it, and flushes into the
//! [`search::tantivy_backend::CrossDocumentIndex`]. File-change invalidation
//! is mtime+length based (a full content hash would be exact but this
//! codebase has no existing hashing dependency to justify pulling one in for
//! this alone — ponytail: coarser signal, upgrade to a hash if a corpus ever
//! shows false-negative skips).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use search::cross_document::IndexStaging;
use search::tantivy_backend::{flush_staged, CrossDocumentIndex, TantivyBackendError};

use crate::broker::IndexEnrollmentRegistry;
use crate::document::DocumentCoordinator;

const MAX_TRACKED_FILES: usize = 65_536;
const MAX_PATH_BYTES: usize = 4096;
const STAGING_BUDGET_BYTES: usize = 8 * 1024 * 1024;
const REGISTRY_MAGIC: &[u8; 8] = b"IDXFILE\0";

/// Per-file bookkeeping: stable source id + last-indexed fingerprint, so
/// unchanged files are skipped and revision-keyed replace stays correct
/// across restarts. [ADR-019 §3]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileFingerprint {
    source: [u8; 16],
    mtime_secs: u64,
    len: u64,
}

/// Bounded path -> fingerprint registry (persisted — see [`save_registry`]),
/// so source ids and last-indexed state survive restart. [ADR-019 §3]
#[derive(Debug, Default)]
pub struct FileIndexRegistry {
    entries: HashMap<PathBuf, FileFingerprint>,
}

/// Fail-closed file-index registry error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileIndexRegistryError {
    /// The bounded registry is full.
    RegistryFull,
    /// OS entropy was unavailable.
    Entropy,
}

impl FileIndexRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Stable opaque source id for `path`, assigning one on first sight.
    fn source_for(&mut self, path: &Path) -> Result<[u8; 16], FileIndexRegistryError> {
        if let Some(fp) = self.entries.get(path) {
            return Ok(fp.source);
        }
        if self.entries.len() >= MAX_TRACKED_FILES {
            return Err(FileIndexRegistryError::RegistryFull);
        }
        let mut id = [0u8; 16];
        getrandom::fill(&mut id).map_err(|_| FileIndexRegistryError::Entropy)?;
        self.entries.insert(
            path.to_path_buf(),
            FileFingerprint {
                source: id,
                mtime_secs: 0,
                len: 0,
            },
        );
        Ok(id)
    }

    /// Whether `path` has changed since it was last indexed (or was never
    /// indexed). [ADR-019 §3 file-change invalidation]
    fn needs_reindex(&self, path: &Path, mtime_secs: u64, len: u64) -> bool {
        match self.entries.get(path) {
            Some(fp) => fp.mtime_secs != mtime_secs || fp.len != len,
            None => true,
        }
    }

    fn record_indexed(&mut self, path: &Path, source: [u8; 16], mtime_secs: u64, len: u64) {
        self.entries.insert(
            path.to_path_buf(),
            FileFingerprint {
                source,
                mtime_secs,
                len,
            },
        );
    }

    /// Drop bookkeeping for `path` (deletion visibility). Caller must also
    /// call [`CrossDocumentIndex::remove_source`] — see [`remove_file`].
    pub fn forget(&mut self, path: &Path) -> Option<[u8; 16]> {
        self.entries.remove(path).map(|fp| fp.source)
    }

    /// Every currently tracked path, for settings visibility.
    pub fn tracked_paths(&self) -> impl Iterator<Item = &Path> {
        self.entries.keys().map(PathBuf::as_path)
    }

    /// Number of currently tracked files, for settings visibility.
    pub fn tracked_file_count(&self) -> usize {
        self.entries.len()
    }
}

/// Persist the registry so source ids and last-indexed fingerprints survive
/// restart (resume — ADR-019 §3). Full-rewrite, not append-log: this
/// registry is small (bounded by [`MAX_TRACKED_FILES`]) compared to the
/// index itself, and a torn write just means the next scan re-indexes
/// everything once — safe, just slower, not incorrect.
pub fn save_registry(registry: &FileIndexRegistry, path: &Path) -> std::io::Result<()> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(REGISTRY_MAGIC);
    bytes.extend_from_slice(&(registry.entries.len() as u32).to_le_bytes());
    for (file_path, fp) in &registry.entries {
        let path_bytes = file_path.to_string_lossy().into_owned().into_bytes();
        bytes.extend_from_slice(&(path_bytes.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&path_bytes);
        bytes.extend_from_slice(&fp.source);
        bytes.extend_from_slice(&fp.mtime_secs.to_le_bytes());
        bytes.extend_from_slice(&fp.len.to_le_bytes());
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, &bytes)?;
    std::fs::rename(&tmp, path)
}

/// Load a persisted registry, or an empty one if none exists yet.
pub fn load_registry(path: &Path) -> std::io::Result<FileIndexRegistry> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(FileIndexRegistry::new())
        }
        Err(error) => return Err(error),
    };
    let malformed = || std::io::Error::new(std::io::ErrorKind::InvalidData, "malformed registry");
    if bytes.len() < 12 || &bytes[..8] != REGISTRY_MAGIC {
        return Err(malformed());
    }
    let count = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    if count > MAX_TRACKED_FILES {
        return Err(malformed());
    }
    let mut offset = 12usize;
    let mut entries = HashMap::with_capacity(count);
    for _ in 0..count {
        if bytes.len() < offset + 4 {
            return Err(malformed());
        }
        let path_len =
            u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;
        if path_len > MAX_PATH_BYTES || bytes.len() < offset + path_len + 16 + 8 + 8 {
            return Err(malformed());
        }
        let path_str = std::str::from_utf8(&bytes[offset..offset + path_len])
            .map_err(|_| malformed())?
            .to_owned();
        offset += path_len;
        let mut source = [0u8; 16];
        source.copy_from_slice(&bytes[offset..offset + 16]);
        offset += 16;
        let mtime_secs = u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap());
        offset += 8;
        let len = u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap());
        offset += 8;
        entries.insert(
            PathBuf::from(path_str),
            FileFingerprint {
                source,
                mtime_secs,
                len,
            },
        );
    }
    Ok(FileIndexRegistry { entries })
}

/// One enrollment reindex pass outcome.
#[derive(Debug, Default)]
pub struct ReindexReport {
    /// `.pdf` files found under the root and authorized against the enrollment.
    pub files_scanned: usize,
    /// Files whose content changed (or were never seen) and were reindexed.
    pub files_reindexed: usize,
    /// Files unchanged since their last index pass — skipped.
    pub files_skipped_unchanged: usize,
    /// Total pages staged+flushed across all reindexed files.
    pub pages_indexed: usize,
    /// Per-file failures (file kept its previous index state, not removed).
    pub errors: Vec<(PathBuf, String)>,
}

/// Walk `root` recursively, authorize each `.pdf` file against `enrollment`,
/// and reindex any that changed since the last pass. [ADR-019 §3]
pub fn reindex_enrollment(
    worker_exe: &Path,
    enrollment_registry: &IndexEnrollmentRegistry,
    enrollment: [u8; 16],
    root: &Path,
    file_registry: &mut FileIndexRegistry,
    index: &mut CrossDocumentIndex,
) -> ReindexReport {
    let mut report = ReindexReport::default();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(error) => {
                report.errors.push((dir, error.to_string()));
                continue;
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("pdf") {
                continue;
            }
            let authorized = match enrollment_registry.authorize(enrollment, &path) {
                Ok(p) => p,
                Err(error) => {
                    report.errors.push((path, format!("{error:?}")));
                    continue;
                }
            };
            report.files_scanned += 1;
            let metadata = match std::fs::metadata(&authorized) {
                Ok(m) => m,
                Err(error) => {
                    report.errors.push((authorized, error.to_string()));
                    continue;
                }
            };
            let mtime_secs = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let len = metadata.len();
            if !file_registry.needs_reindex(&authorized, mtime_secs, len) {
                report.files_skipped_unchanged += 1;
                continue;
            }
            let source = match file_registry.source_for(&authorized) {
                Ok(s) => s,
                Err(error) => {
                    report.errors.push((authorized, format!("{error:?}")));
                    continue;
                }
            };
            match reindex_one_file(worker_exe, &authorized, source, index) {
                Ok(pages) => {
                    report.files_reindexed += 1;
                    report.pages_indexed += pages;
                    file_registry.record_indexed(&authorized, source, mtime_secs, len);
                }
                Err(message) => report.errors.push((authorized, message)),
            }
        }
    }
    report
}

fn reindex_one_file(
    worker_exe: &Path,
    path: &Path,
    source: [u8; 16],
    index: &mut CrossDocumentIndex,
) -> Result<usize, String> {
    let mut coord =
        DocumentCoordinator::open(worker_exe, path).map_err(|e| format!("open failed: {e}"))?;
    let page_count = coord.page_count();
    let revision = coord.revision();
    let mut staging = IndexStaging::new(STAGING_BUDGET_BYTES)
        .map_err(|e| format!("staging init failed: {e:?}"))?;
    for page in 0..page_count {
        let model = coord
            .get_page_text(page)
            .map_err(|error| format!("page {page} extract failed: {error}"))?;
        staging
            .ingest(source, revision, model)
            .map_err(|error| format!("page {page} staging failed: {error:?}"))?;
    }
    let _ = coord.close();
    flush_staged(&mut staging, index).map_err(|error| format!("index flush failed: {error}"))?;
    Ok(page_count as usize)
}

/// Enrolled-index summary for settings surfacing (size-budgeted,
/// inspectable — ADR-019 §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndexingSummary {
    /// Number of files currently tracked in the index.
    pub tracked_file_count: usize,
    /// Approximate on-disk index size in bytes.
    pub disk_size_bytes: u64,
}

/// Build the settings-visibility summary for one index directory.
pub fn indexing_summary(
    file_registry: &FileIndexRegistry,
    index: &CrossDocumentIndex,
    index_dir: &Path,
) -> IndexingSummary {
    IndexingSummary {
        tracked_file_count: file_registry.tracked_file_count(),
        disk_size_bytes: index.disk_size_bytes(index_dir),
    }
}

/// Remove one file's documents from the index and forget its fingerprint
/// (deletion visibility / explicit user delete). Returns `false` if the
/// file was never tracked. [ADR-019 §3]
pub fn remove_file(
    file_registry: &mut FileIndexRegistry,
    index: &mut CrossDocumentIndex,
    path: &Path,
) -> Result<bool, TantivyBackendError> {
    match file_registry.forget(path) {
        Some(source) => {
            index.remove_source(source)?;
            Ok(true)
        }
        None => Ok(false),
    }
}

/// Remove every tracked file under `root` (enrollment removal). Returns the
/// number of files removed. [ADR-019 §3]
pub fn remove_enrollment_files(
    file_registry: &mut FileIndexRegistry,
    index: &mut CrossDocumentIndex,
    root: &Path,
) -> Result<usize, TantivyBackendError> {
    // Tracked paths are canonicalized (see `IndexEnrollmentRegistry::authorize`,
    // called before anything is tracked) — `root` must be too, or `starts_with`
    // silently never matches (e.g. Windows prepends `\\?\` on canonicalize).
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let to_remove: Vec<PathBuf> = file_registry
        .tracked_paths()
        .filter(|p| p.starts_with(&root))
        .map(PathBuf::from)
        .collect();
    let mut removed = 0;
    for path in to_remove {
        if remove_file(file_registry, index, &path)? {
            removed += 1;
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_for_is_stable_and_bounded() {
        let mut registry = FileIndexRegistry::new();
        let path = Path::new("/enrolled/a.pdf");
        let first = registry.source_for(path).unwrap();
        let second = registry.source_for(path).unwrap();
        assert_eq!(first, second, "same path must keep the same source id");
    }

    #[test]
    fn needs_reindex_true_until_recorded_then_false_until_changed() {
        let mut registry = FileIndexRegistry::new();
        let path = Path::new("/enrolled/a.pdf");
        assert!(registry.needs_reindex(path, 100, 50));
        let source = registry.source_for(path).unwrap();
        registry.record_indexed(path, source, 100, 50);
        assert!(!registry.needs_reindex(path, 100, 50));
        assert!(
            registry.needs_reindex(path, 200, 50),
            "changed mtime must trigger reindex"
        );
        assert!(
            registry.needs_reindex(path, 100, 999),
            "changed length must trigger reindex"
        );
    }

    #[test]
    fn forget_removes_and_returns_the_source() {
        let mut registry = FileIndexRegistry::new();
        let path = Path::new("/enrolled/a.pdf");
        let source = registry.source_for(path).unwrap();
        assert_eq!(registry.forget(path), Some(source));
        assert_eq!(registry.forget(path), None, "second forget is a no-op");
        assert!(registry.needs_reindex(path, 0, 0));
    }

    #[test]
    fn registry_persists_across_save_and_load() {
        let mut registry = FileIndexRegistry::new();
        let path_a = Path::new("/enrolled/a.pdf");
        let path_b = Path::new("/enrolled/sub/b.pdf");
        let source_a = registry.source_for(path_a).unwrap();
        let source_b = registry.source_for(path_b).unwrap();
        registry.record_indexed(path_a, source_a, 111, 222);
        registry.record_indexed(path_b, source_b, 333, 444);

        let file = std::env::temp_dir().join(format!(
            "pdf-platform-file-index-registry-{}.bin",
            std::process::id()
        ));
        save_registry(&registry, &file).unwrap();
        let loaded = load_registry(&file).unwrap();

        assert_eq!(loaded.tracked_file_count(), 2);
        assert!(!loaded.needs_reindex(path_a, 111, 222));
        assert!(!loaded.needs_reindex(path_b, 333, 444));
        assert!(loaded.needs_reindex(path_a, 999, 222));
        let _ = std::fs::remove_file(file);
    }

    #[test]
    fn load_registry_missing_file_returns_empty() {
        let file = std::env::temp_dir().join(format!(
            "pdf-platform-file-index-registry-missing-{}.bin",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&file);
        let loaded = load_registry(&file).unwrap();
        assert_eq!(loaded.tracked_file_count(), 0);
    }
}
