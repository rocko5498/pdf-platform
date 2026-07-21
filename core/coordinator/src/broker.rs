//! Document broker: privileged file open in Z0. [SDS §2.2.6, §3.1 step 2, ADR-016]
//!
//! Lower zones must not open arbitrary paths; the broker validates and opens.

use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use jobs::utility_pool::UtilityWorkerIdentity;

const MAX_UTILITY_GRANTS: usize = 4096;
const MAX_INDEX_ENROLLMENTS: usize = 256;

/// Fail-closed cross-document indexing enrollment error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexEnrollmentError {
    /// The selected root is missing or is not a directory.
    InvalidRoot,
    /// The bounded enrollment registry is full.
    RegistryFull,
    /// OS entropy was unavailable or repeatedly collided.
    Entropy(String),
    /// Enrollment identifier was never issued or was removed.
    Unknown,
    /// Candidate file resolves outside the explicitly enrolled root.
    OutsideEnrollment,
    /// Candidate resolves inside the root but is not a regular file.
    NotFile,
}

/// Z0-only registry of explicitly enrolled indexing roots.
#[derive(Default)]
pub struct IndexEnrollmentRegistry {
    roots: HashMap<[u8; 16], PathBuf>,
}

impl IndexEnrollmentRegistry {
    /// Create an empty enrollment registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Canonicalize and enroll one user-selected directory.
    pub fn enroll(&mut self, root: &Path) -> Result<[u8; 16], IndexEnrollmentError> {
        if self.roots.len() >= MAX_INDEX_ENROLLMENTS {
            return Err(IndexEnrollmentError::RegistryFull);
        }
        let root = root
            .canonicalize()
            .map_err(|_| IndexEnrollmentError::InvalidRoot)?;
        if !root.is_dir() {
            return Err(IndexEnrollmentError::InvalidRoot);
        }
        for _ in 0..4 {
            let mut id = [0; 16];
            getrandom::fill(&mut id)
                .map_err(|error| IndexEnrollmentError::Entropy(error.to_string()))?;
            if let std::collections::hash_map::Entry::Vacant(entry) = self.roots.entry(id) {
                entry.insert(root.clone());
                return Ok(id);
            }
        }
        Err(IndexEnrollmentError::Entropy(
            "repeated enrollment identifier collision".into(),
        ))
    }

    /// Authorize one existing regular file under an active enrollment.
    pub fn authorize(
        &self,
        enrollment: [u8; 16],
        candidate: &Path,
    ) -> Result<PathBuf, IndexEnrollmentError> {
        let root = self
            .roots
            .get(&enrollment)
            .ok_or(IndexEnrollmentError::Unknown)?;
        let candidate = candidate
            .canonicalize()
            .map_err(|_| IndexEnrollmentError::OutsideEnrollment)?;
        if !candidate.starts_with(root) {
            return Err(IndexEnrollmentError::OutsideEnrollment);
        }
        if !candidate.is_file() {
            return Err(IndexEnrollmentError::NotFile);
        }
        Ok(candidate)
    }

    /// Remove an enrollment and deny future indexing under it.
    pub fn remove(&mut self, enrollment: [u8; 16]) -> bool {
        self.roots.remove(&enrollment).is_some()
    }

    /// Every currently enrolled `(id, root)` pair, for settings visibility
    /// and CLI listing.
    pub fn enrollments(&self) -> impl Iterator<Item = ([u8; 16], &Path)> {
        self.roots.iter().map(|(id, root)| (*id, root.as_path()))
    }
}

const ENROLLMENT_REGISTRY_MAGIC: &[u8; 8] = b"IDXENRL\0";
const MAX_ENROLLMENT_PATH_BYTES: usize = 4096;

/// Persist enrolled roots so they survive across CLI invocations / restart
/// (a fresh process otherwise starts with an empty, in-memory-only
/// registry — enrollment would be useless if it didn't outlive one
/// process). Full-rewrite, matching `coordinator::indexing`'s file
/// registry persistence style: a torn write just means re-enrolling next
/// time, not data corruption.
pub fn save_enrollment_registry(
    registry: &IndexEnrollmentRegistry,
    path: &Path,
) -> io::Result<()> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(ENROLLMENT_REGISTRY_MAGIC);
    bytes.extend_from_slice(&(registry.roots.len() as u32).to_le_bytes());
    for (id, root) in &registry.roots {
        let root_bytes = root.to_string_lossy().into_owned().into_bytes();
        bytes.extend_from_slice(id);
        bytes.extend_from_slice(&(root_bytes.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&root_bytes);
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, &bytes)?;
    std::fs::rename(&tmp, path)
}

/// Load a persisted enrollment registry, or an empty one if none exists yet.
pub fn load_enrollment_registry(path: &Path) -> io::Result<IndexEnrollmentRegistry> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(IndexEnrollmentRegistry::new())
        }
        Err(error) => return Err(error),
    };
    let malformed = || io::Error::new(io::ErrorKind::InvalidData, "malformed enrollment registry");
    if bytes.len() < 12 || &bytes[..8] != ENROLLMENT_REGISTRY_MAGIC {
        return Err(malformed());
    }
    let count = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    if count > MAX_INDEX_ENROLLMENTS {
        return Err(malformed());
    }
    let mut offset = 12usize;
    let mut roots = HashMap::with_capacity(count);
    for _ in 0..count {
        if bytes.len() < offset + 16 + 4 {
            return Err(malformed());
        }
        let mut id = [0u8; 16];
        id.copy_from_slice(&bytes[offset..offset + 16]);
        offset += 16;
        let path_len = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;
        if path_len > MAX_ENROLLMENT_PATH_BYTES || bytes.len() < offset + path_len {
            return Err(malformed());
        }
        let root_str = std::str::from_utf8(&bytes[offset..offset + path_len])
            .map_err(|_| malformed())?
            .to_owned();
        offset += path_len;
        roots.insert(id, PathBuf::from(root_str));
    }
    Ok(IndexEnrollmentRegistry { roots })
}

/// Capability attached to an opaque utility-worker grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UtilityGrantKind {
    /// Worker may read from a bounded shared-memory region.
    SharedMemoryRead,
    /// Worker may write to a bounded shared-memory region.
    SharedMemoryWrite,
}

/// Fail-closed utility grant validation error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UtilityGrantError {
    /// Grant registry reached its declared bound.
    RegistryFull,
    /// OS entropy was unavailable.
    Entropy(String),
    /// Expiry instant could not be represented.
    ExpiryOverflow,
    /// Identifier was never issued or has been revoked.
    Unknown,
    /// Grant has expired.
    Expired,
    /// Requested capability differs from the issued capability.
    WrongCapability,
    /// Grant belongs to a different scheduler job.
    WrongJob,
    /// Grant belongs to a worker process that has been replaced.
    WrongWorkerIdentity,
    /// Requested byte range exceeds the grant.
    OutOfBounds,
}

#[derive(Debug, Clone, Copy)]
struct UtilityGrant {
    kind: UtilityGrantKind,
    job_id: u64,
    worker: UtilityWorkerIdentity,
    byte_len: u64,
    expires_at: Instant,
}

/// Z0-owned bounded registry for opaque utility-worker capabilities.
#[derive(Default)]
pub struct UtilityGrantRegistry {
    grants: HashMap<[u8; 16], UtilityGrant>,
}

impl UtilityGrantRegistry {
    /// Create an empty grant registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Issue a random grant bound to one job and worker generation.
    pub fn issue(
        &mut self,
        kind: UtilityGrantKind,
        job_id: u64,
        worker: UtilityWorkerIdentity,
        byte_len: u64,
        ttl: Duration,
    ) -> Result<[u8; 16], UtilityGrantError> {
        self.issue_at(kind, job_id, worker, byte_len, ttl, Instant::now())
    }

    fn issue_at(
        &mut self,
        kind: UtilityGrantKind,
        job_id: u64,
        worker: UtilityWorkerIdentity,
        byte_len: u64,
        ttl: Duration,
        now: Instant,
    ) -> Result<[u8; 16], UtilityGrantError> {
        if self.grants.len() >= MAX_UTILITY_GRANTS {
            return Err(UtilityGrantError::RegistryFull);
        }
        let expires_at = now
            .checked_add(ttl)
            .ok_or(UtilityGrantError::ExpiryOverflow)?;
        for _ in 0..4 {
            let mut id = [0; 16];
            getrandom::fill(&mut id)
                .map_err(|error| UtilityGrantError::Entropy(error.to_string()))?;
            if let std::collections::hash_map::Entry::Vacant(entry) = self.grants.entry(id) {
                entry.insert(UtilityGrant {
                    kind,
                    job_id,
                    worker,
                    byte_len,
                    expires_at,
                });
                return Ok(id);
            }
        }
        Err(UtilityGrantError::Entropy(
            "repeated opaque grant collision".into(),
        ))
    }

    /// Validate capability, ownership, generation, expiry, and byte bounds.
    pub fn validate(
        &mut self,
        id: [u8; 16],
        kind: UtilityGrantKind,
        job_id: u64,
        worker: UtilityWorkerIdentity,
        offset: u64,
        length: u64,
    ) -> Result<(), UtilityGrantError> {
        self.validate_at(id, kind, job_id, worker, offset, length, Instant::now())
    }

    #[allow(clippy::too_many_arguments)]
    fn validate_at(
        &mut self,
        id: [u8; 16],
        kind: UtilityGrantKind,
        job_id: u64,
        worker: UtilityWorkerIdentity,
        offset: u64,
        length: u64,
        now: Instant,
    ) -> Result<(), UtilityGrantError> {
        let grant = self
            .grants
            .get(&id)
            .copied()
            .ok_or(UtilityGrantError::Unknown)?;
        if now >= grant.expires_at {
            self.grants.remove(&id);
            return Err(UtilityGrantError::Expired);
        }
        if grant.kind != kind {
            return Err(UtilityGrantError::WrongCapability);
        }
        if grant.job_id != job_id {
            return Err(UtilityGrantError::WrongJob);
        }
        if grant.worker != worker {
            return Err(UtilityGrantError::WrongWorkerIdentity);
        }
        let end = offset
            .checked_add(length)
            .ok_or(UtilityGrantError::OutOfBounds)?;
        if end > grant.byte_len {
            return Err(UtilityGrantError::OutOfBounds);
        }
        Ok(())
    }

    /// Revoke all capabilities held by a worker process before replacement.
    pub fn revoke_worker(&mut self, worker: UtilityWorkerIdentity) -> usize {
        let before = self.grants.len();
        self.grants.retain(|_, grant| grant.worker != worker);
        before - self.grants.len()
    }
}

/// Build the pool replacement hook that revokes every grant for the old process identity.
pub fn utility_grant_revocation_hook(
    registry: Arc<Mutex<UtilityGrantRegistry>>,
) -> impl Fn(UtilityWorkerIdentity) + Send + Sync + 'static {
    move |worker| {
        if let Ok(mut registry) = registry.lock() {
            registry.revoke_worker(worker);
        }
    }
}

/// A document file opened by the broker for a session.
///
/// Holds the OS `File` (inherited into the worker as FD/HANDLE) and the path
/// for Z0 identity / logging only — path is not sent to Z1. [GR-1, handle-inherit]
pub struct BrokeredFile {
    path: PathBuf,
    file: File,
}

impl BrokeredFile {
    /// Path that was opened (Z0 only; not passed to the worker).
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Borrow the opened file (read-only intent; re-inherited on respawn).
    pub fn file(&self) -> &File {
        &self.file
    }
}

/// Open a document path read-only after basic validation. [SDS §3.1 step 2]
pub fn open_read_only(path: &Path) -> io::Result<BrokeredFile> {
    let meta = std::fs::metadata(path)?;
    if !meta.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path is not a regular file",
        ));
    }
    let file = File::open(path)?;
    Ok(BrokeredFile {
        path: path.to_path_buf(),
        file,
    })
}

/// Z0-owned optimization candidate created beside its final destination.
///
/// The temporary path never crosses into Z1; workers receive only [`Self::file_mut`]'s
/// inherited OS handle. Dropping an unpublished candidate removes it.
pub struct BrokeredOptimizationCandidate {
    id: [u8; 16],
    path: PathBuf,
    destination: PathBuf,
    file: Option<File>,
    published: bool,
}

/// Proof that the coordinator verified one exact optimization candidate.
pub struct VerifiedOptimizationCandidate {
    id: [u8; 16],
}

impl BrokeredOptimizationCandidate {
    /// Mutable candidate handle for serialization or inheritance into Z1.
    pub fn file_mut(&mut self) -> &mut File {
        self.file.as_mut().expect("candidate file remains open")
    }

    /// Flush candidate bytes and run coordinator-owned structural/conformance checks.
    pub fn verify_with(
        &mut self,
        verifier: impl FnOnce(&File) -> io::Result<()>,
    ) -> io::Result<VerifiedOptimizationCandidate> {
        let file = self.file.as_ref().expect("candidate file remains open");
        file.sync_all()?;
        verifier(file)?;
        Ok(VerifiedOptimizationCandidate { id: self.id })
    }

    /// Flush candidate bytes and atomically publish to a new destination.
    ///
    /// Existing destinations are never overwritten by this path.
    pub fn publish_verified(
        mut self,
        verification: VerifiedOptimizationCandidate,
    ) -> io::Result<()> {
        if verification.id != self.id {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "verification belongs to a different candidate",
            ));
        }
        let file = self.file.take().expect("candidate file remains open");
        file.sync_all()?;
        drop(file);
        if self.destination.exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "optimization destination already exists",
            ));
        }
        std::fs::rename(&self.path, &self.destination)?;
        self.published = true;
        Ok(())
    }
}

impl Drop for BrokeredOptimizationCandidate {
    fn drop(&mut self) {
        if !self.published {
            self.file.take();
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Create a unique candidate in the destination directory for atomic publication.
pub fn create_optimization_candidate(
    destination: &Path,
) -> io::Result<BrokeredOptimizationCandidate> {
    let parent = destination
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "destination has no parent"))?;
    if !parent.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "destination directory does not exist",
        ));
    }
    for _ in 0..4 {
        let mut random = [0; 16];
        getrandom::fill(&mut random).map_err(io::Error::other)?;
        let suffix: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
        let path = parent.join(format!(".pdf-platform-candidate-{suffix}.tmp"));
        match File::options()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => {
                return Ok(BrokeredOptimizationCandidate {
                    id: random,
                    path,
                    destination: destination.to_path_buf(),
                    file: Some(file),
                    published: false,
                });
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "repeated optimization candidate collision",
    ))
}

/// Run a full-rewrite generator through the candidate/verify/publish safety
/// net. [ADR-012, ADR-016, ADR-021]
///
/// `generate` receives the candidate's path (not the real destination) and
/// writes the rewritten document there by whatever means it uses internally
/// (e.g. `assembly_ops::optimize_pdf` shelling out to `qpdf`) — this
/// function does not care how the bytes get written, only that they get
/// verified before anything is published. It does *not* add Z1 sandboxing
/// around `generate` itself: today's `optimize_pdf` already runs directly in
/// the CLI/Z0 process (unsandboxed) and continues to do so here — this adds
/// the missing safety net (nothing was verified before overwriting the
/// destination previously), not new process isolation. A from-scratch
/// in-process Rust rewriter — matching this codebase's own stated
/// "qpdf as the correctness reference [for testing], not the production
/// writer" principle — is a separate, much larger undertaking.
///
/// SECURITY: requires human broker review before merge.
pub fn optimize_with_verification(
    destination: &Path,
    generate: impl FnOnce(&Path) -> Result<String, String>,
) -> Result<String, String> {
    let mut candidate =
        create_optimization_candidate(destination).map_err(|e| format!("candidate create failed: {e}"))?;
    // Release our handle before handing the path to an external generator
    // (e.g. qpdf): on Windows, a second process opening the same path while
    // we still hold it can hit a sharing violation even though our handle
    // requests shared read/write access — verified empirically, not just in
    // theory, running `optimize` through the real CLI against real qpdf.
    candidate.file = None;
    let report = generate(&candidate.path)?;

    // Fsync via our own handle, then fully release it again before spawning
    // qpdf as a separate process against the same path — same sharing-rule
    // reason as above; `verify_with`'s built-in fsync-then-verify sequencing
    // can't be reused here because it keeps our handle open across the
    // verifier call, which is exactly the case that fails.
    {
        let file = File::options()
            .read(true)
            .write(true)
            .open(&candidate.path)
            .map_err(|e| format!("candidate reopen for fsync failed: {e}"))?;
        file.sync_all()
            .map_err(|e| format!("candidate fsync failed: {e}"))?;
    }

    // Verify with qpdf itself, not our own `pdf_cos` scanner: this project's
    // own architecture doc says structural ownership is ours "with qpdf as
    // the correctness reference" — and empirically, `pdf_cos::scan` doesn't
    // yet parse the xref-stream format qpdf's own `--object-streams=generate`
    // output uses, which would falsely reject perfectly valid optimizer
    // output. That's a separate, real `pdf-cos` gap, not something to chase
    // down mid-verification-layer; qpdf is the more authoritative check here
    // regardless.
    let status = std::process::Command::new("qpdf")
        .arg("--check")
        .arg(&candidate.path)
        .status()
        .map_err(|e| format!("qpdf --check spawn failed: {e}"))?;
    // qpdf's own exit-code convention: 0 = clean, 3 = usable with warnings
    // (e.g. a repairable minor issue), anything else = real damage/errors.
    if !matches!(status.code(), Some(0) | Some(3)) {
        return Err(format!(
            "candidate verification failed: qpdf --check reported problems (exit {status})"
        ));
    }

    candidate.file = Some(
        File::options()
            .read(true)
            .write(true)
            .open(&candidate.path)
            .map_err(|e| format!("candidate reopen for publish failed: {e}"))?,
    );
    let verification = VerifiedOptimizationCandidate { id: candidate.id };
    candidate
        .publish_verified(verification)
        .map_err(|e| format!("publish failed: {e}"))?;
    Ok(report)
}

#[cfg(test)]
fn minimal_pdf(page_count: u32) -> Vec<u8> {
    let mut bytes = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    offsets.push(bytes.len());
    bytes.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
    offsets.push(bytes.len());
    if page_count == 0 {
        bytes.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Kids [] /Count 0 >>\nendobj\n");
    } else {
        let kids: String = (0..page_count)
            .map(|i| format!("{} 0 R", i + 3))
            .collect::<Vec<_>>()
            .join(" ");
        bytes.extend_from_slice(
            format!("2 0 obj\n<< /Type /Pages /Kids [{kids}] /Count {page_count} >>\nendobj\n")
                .as_bytes(),
        );
        for i in 0..page_count {
            offsets.push(bytes.len());
            bytes.extend_from_slice(
                format!("{} 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>\nendobj\n", i + 3)
                    .as_bytes(),
            );
        }
    }
    let xref = bytes.len();
    bytes.extend_from_slice(format!("xref\n0 {}\n", offsets.len() + 1).as_bytes());
    bytes.extend_from_slice(b"0000000000 65535 f \n");
    for offset in &offsets {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            offsets.len() + 1
        )
        .as_bytes(),
    );
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn optimize_with_verification_publishes_valid_generated_output() {
        if !pdf_model::assembly_ops::qpdf_available() {
            return;
        }
        let base = std::env::temp_dir().join(format!(
            "pdf-platform-optimize-verify-ok-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&base).unwrap();
        let destination = base.join("optimized.pdf");

        let report = optimize_with_verification(&destination, |candidate_path| {
            std::fs::write(candidate_path, minimal_pdf(2)).map_err(|e| e.to_string())?;
            Ok("optimized: 2 pages".to_string())
        })
        .unwrap();

        assert_eq!(report, "optimized: 2 pages");
        assert!(destination.exists(), "verified output must be published");
        assert_eq!(&std::fs::read(&destination).unwrap()[..5], b"%PDF-");
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn optimize_with_verification_never_publishes_when_generation_fails() {
        let base = std::env::temp_dir().join(format!(
            "pdf-platform-optimize-verify-genfail-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&base).unwrap();
        let destination = base.join("optimized.pdf");

        let result = optimize_with_verification(&destination, |_candidate_path| {
            Err("simulated qpdf crash".to_string())
        });

        assert_eq!(result, Err("simulated qpdf crash".to_string()));
        assert!(!destination.exists());
        let leftover = std::fs::read_dir(&base).unwrap().count();
        assert_eq!(leftover, 0, "failed-generation candidate must self-delete");
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn optimize_with_verification_never_publishes_a_structurally_invalid_candidate() {
        if !pdf_model::assembly_ops::qpdf_available() {
            return;
        }
        let base = std::env::temp_dir().join(format!(
            "pdf-platform-optimize-verify-badstruct-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&base).unwrap();
        let destination = base.join("optimized.pdf");

        let result = optimize_with_verification(&destination, |candidate_path| {
            // Simulates a generator that crashed mid-write, leaving garbage
            // instead of a parseable PDF — exactly what `qpdf --check` (not
            // our own less-mature `pdf_cos` scanner) is well-suited to catch.
            std::fs::write(candidate_path, b"not a pdf at all, just garbage bytes")
                .map_err(|e| e.to_string())?;
            Ok("optimized: garbage".to_string())
        });

        assert!(result.is_err(), "garbage candidate must fail verification");
        assert!(!destination.exists(), "unverified candidate must never be published");
        let leftover = std::fs::read_dir(&base).unwrap().count();
        assert_eq!(leftover, 0, "rejected candidate must self-delete");
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn optimize_with_verification_never_overwrites_an_existing_destination() {
        if !pdf_model::assembly_ops::qpdf_available() {
            return;
        }
        let base = std::env::temp_dir().join(format!(
            "pdf-platform-optimize-verify-exists-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&base).unwrap();
        let destination = base.join("optimized.pdf");
        std::fs::write(&destination, b"pre-existing user file").unwrap();

        let result = optimize_with_verification(&destination, |candidate_path| {
            std::fs::write(candidate_path, minimal_pdf(1)).map_err(|e| e.to_string())?;
            Ok("optimized: 1 page".to_string())
        });

        assert!(result.is_err(), "must refuse to publish over an existing file");
        assert_eq!(
            std::fs::read(&destination).unwrap(),
            b"pre-existing user file",
            "existing destination must be untouched"
        );
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn optimization_candidate_never_changes_source_before_verified_publish() {
        let base =
            std::env::temp_dir().join(format!("pdf-platform-candidate-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let source = base.join("source.pdf");
        let destination = base.join("optimized.pdf");
        std::fs::write(&source, b"original").unwrap();

        {
            let mut candidate = create_optimization_candidate(&destination).unwrap();
            candidate.file_mut().write_all(b"candidate").unwrap();
        }
        assert_eq!(std::fs::read(&source).unwrap(), b"original");
        assert!(!destination.exists());

        let mut candidate = create_optimization_candidate(&destination).unwrap();
        candidate.file_mut().write_all(b"verified").unwrap();
        let verification = candidate
            .verify_with(|file| {
                if file.metadata()?.len() != 8 {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "wrong length"));
                }
                Ok(())
            })
            .unwrap();
        candidate.publish_verified(verification).unwrap();
        assert_eq!(std::fs::read(&source).unwrap(), b"original");
        assert_eq!(std::fs::read(&destination).unwrap(), b"verified");
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn optimization_verification_cannot_authorize_another_candidate() {
        let base = std::env::temp_dir().join(format!(
            "pdf-platform-candidate-token-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&base).unwrap();
        let mut first = create_optimization_candidate(&base.join("first.pdf")).unwrap();
        first.file_mut().write_all(b"first").unwrap();
        let verification = first.verify_with(|_| Ok(())).unwrap();
        let second = create_optimization_candidate(&base.join("second.pdf")).unwrap();

        assert_eq!(
            second.publish_verified(verification).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert!(!base.join("second.pdf").exists());
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn index_enrollment_denies_files_outside_explicit_root() {
        let base = std::env::temp_dir().join(format!(
            "pdf-platform-index-enrollment-{}",
            std::process::id()
        ));
        let enrolled = base.join("enrolled");
        let outside = base.join("outside");
        std::fs::create_dir_all(&enrolled).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let allowed_file = enrolled.join("allowed.pdf");
        let denied_file = outside.join("denied.pdf");
        std::fs::write(&allowed_file, b"allowed").unwrap();
        std::fs::write(&denied_file, b"denied").unwrap();

        let mut registry = IndexEnrollmentRegistry::new();
        let enrollment = registry.enroll(&enrolled).unwrap();
        assert!(registry.authorize(enrollment, &allowed_file).is_ok());
        assert_eq!(
            registry.authorize(enrollment, &denied_file),
            Err(IndexEnrollmentError::OutsideEnrollment)
        );
        assert_eq!(
            registry.authorize([0; 16], &allowed_file),
            Err(IndexEnrollmentError::Unknown)
        );

        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn enrollment_registry_persists_across_save_and_load() {
        let base = std::env::temp_dir().join(format!(
            "pdf-platform-enrollment-persist-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&base).unwrap();

        let mut registry = IndexEnrollmentRegistry::new();
        let id = registry.enroll(&base).unwrap();

        let file = base.join("enrollments.bin");
        save_enrollment_registry(&registry, &file).unwrap();
        let loaded = load_enrollment_registry(&file).unwrap();

        let allowed = base.join("doc.pdf");
        std::fs::write(&allowed, b"x").unwrap();
        assert!(loaded.authorize(id, &allowed).is_ok());
        assert_eq!(loaded.enrollments().count(), 1);

        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn load_enrollment_registry_missing_file_returns_empty() {
        let file = std::env::temp_dir().join(format!(
            "pdf-platform-enrollment-missing-{}.bin",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&file);
        let loaded = load_enrollment_registry(&file).unwrap();
        assert_eq!(loaded.enrollments().count(), 0);
    }

    #[test]
    fn open_read_only_temp_file() {
        let dir = std::env::temp_dir();
        let p = dir.join(format!(
            "pdf-platform-broker-test-{}-{}.bin",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        {
            let mut f = File::create(&p).unwrap();
            f.write_all(b"%PDF-1.4 test").unwrap();
        }
        let b = open_read_only(&p).expect("open");
        assert_eq!(b.path(), p.as_path());
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn utility_grants_fail_closed_for_forgery_scope_and_bounds() {
        let mut grants = UtilityGrantRegistry::new();
        let worker = UtilityWorkerIdentity {
            slot: 3,
            generation: 0,
        };
        let grant = grants
            .issue(
                UtilityGrantKind::SharedMemoryRead,
                41,
                worker,
                4096,
                std::time::Duration::from_secs(60),
            )
            .unwrap();
        assert!(grants
            .validate(
                grant,
                UtilityGrantKind::SharedMemoryRead,
                41,
                worker,
                1024,
                2048
            )
            .is_ok());
        assert_eq!(
            grants.validate(
                [0; 16],
                UtilityGrantKind::SharedMemoryRead,
                41,
                worker,
                0,
                1
            ),
            Err(UtilityGrantError::Unknown)
        );
        assert_eq!(
            grants.validate(grant, UtilityGrantKind::SharedMemoryWrite, 41, worker, 0, 1),
            Err(UtilityGrantError::WrongCapability)
        );
        assert_eq!(
            grants.validate(grant, UtilityGrantKind::SharedMemoryRead, 99, worker, 0, 1),
            Err(UtilityGrantError::WrongJob)
        );
        assert_eq!(
            grants.validate(
                grant,
                UtilityGrantKind::SharedMemoryRead,
                41,
                UtilityWorkerIdentity {
                    slot: 4,
                    generation: 0
                },
                0,
                1,
            ),
            Err(UtilityGrantError::WrongWorkerIdentity)
        );
        assert_eq!(
            grants.validate(
                grant,
                UtilityGrantKind::SharedMemoryRead,
                41,
                worker,
                4090,
                32
            ),
            Err(UtilityGrantError::OutOfBounds)
        );
    }

    #[test]
    fn utility_grants_expire_and_worker_replacement_revokes_them() {
        let start = std::time::Instant::now();
        let mut grants = UtilityGrantRegistry::new();
        let worker = UtilityWorkerIdentity {
            slot: 0,
            generation: 7,
        };
        let expired = grants
            .issue_at(
                UtilityGrantKind::SharedMemoryRead,
                1,
                worker,
                10,
                std::time::Duration::from_secs(1),
                start,
            )
            .unwrap();
        assert_eq!(
            grants.validate_at(
                expired,
                UtilityGrantKind::SharedMemoryRead,
                1,
                worker,
                0,
                1,
                start + std::time::Duration::from_secs(2),
            ),
            Err(UtilityGrantError::Expired)
        );
        let live = grants
            .issue_at(
                UtilityGrantKind::SharedMemoryRead,
                2,
                worker,
                10,
                std::time::Duration::from_secs(60),
                start,
            )
            .unwrap();
        assert_eq!(grants.revoke_worker(worker), 1);
        assert_eq!(
            grants.validate_at(
                live,
                UtilityGrantKind::SharedMemoryRead,
                2,
                worker,
                0,
                1,
                start,
            ),
            Err(UtilityGrantError::Unknown)
        );
    }
}
