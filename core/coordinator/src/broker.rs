//! Document broker: privileged file open in Z0. [SDS §2.2.6, §3.1 step 2, ADR-016]
//!
//! Lower zones must not open arbitrary paths; the broker validates and opens.

use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const MAX_UTILITY_GRANTS: usize = 4096;

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
    WrongWorkerGeneration,
    /// Requested byte range exceeds the grant.
    OutOfBounds,
}

#[derive(Debug, Clone, Copy)]
struct UtilityGrant {
    kind: UtilityGrantKind,
    job_id: u64,
    worker_generation: u64,
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
        worker_generation: u64,
        byte_len: u64,
        ttl: Duration,
    ) -> Result<[u8; 16], UtilityGrantError> {
        self.issue_at(
            kind,
            job_id,
            worker_generation,
            byte_len,
            ttl,
            Instant::now(),
        )
    }

    fn issue_at(
        &mut self,
        kind: UtilityGrantKind,
        job_id: u64,
        worker_generation: u64,
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
                    worker_generation,
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
        worker_generation: u64,
        offset: u64,
        length: u64,
    ) -> Result<(), UtilityGrantError> {
        self.validate_at(
            id,
            kind,
            job_id,
            worker_generation,
            offset,
            length,
            Instant::now(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn validate_at(
        &mut self,
        id: [u8; 16],
        kind: UtilityGrantKind,
        job_id: u64,
        worker_generation: u64,
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
        if grant.worker_generation != worker_generation {
            return Err(UtilityGrantError::WrongWorkerGeneration);
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
    pub fn revoke_worker_generation(&mut self, worker_generation: u64) -> usize {
        let before = self.grants.len();
        self.grants
            .retain(|_, grant| grant.worker_generation != worker_generation);
        before - self.grants.len()
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

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
        let grant = grants
            .issue(
                UtilityGrantKind::SharedMemoryRead,
                41,
                3,
                4096,
                std::time::Duration::from_secs(60),
            )
            .unwrap();
        assert!(grants
            .validate(grant, UtilityGrantKind::SharedMemoryRead, 41, 3, 1024, 2048)
            .is_ok());
        assert_eq!(
            grants.validate([0; 16], UtilityGrantKind::SharedMemoryRead, 41, 3, 0, 1),
            Err(UtilityGrantError::Unknown)
        );
        assert_eq!(
            grants.validate(grant, UtilityGrantKind::SharedMemoryWrite, 41, 3, 0, 1),
            Err(UtilityGrantError::WrongCapability)
        );
        assert_eq!(
            grants.validate(grant, UtilityGrantKind::SharedMemoryRead, 99, 3, 0, 1),
            Err(UtilityGrantError::WrongJob)
        );
        assert_eq!(
            grants.validate(grant, UtilityGrantKind::SharedMemoryRead, 41, 4, 0, 1),
            Err(UtilityGrantError::WrongWorkerGeneration)
        );
        assert_eq!(
            grants.validate(grant, UtilityGrantKind::SharedMemoryRead, 41, 3, 4090, 32),
            Err(UtilityGrantError::OutOfBounds)
        );
    }

    #[test]
    fn utility_grants_expire_and_worker_replacement_revokes_them() {
        let start = std::time::Instant::now();
        let mut grants = UtilityGrantRegistry::new();
        let expired = grants
            .issue_at(
                UtilityGrantKind::SharedMemoryRead,
                1,
                7,
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
                7,
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
                7,
                10,
                std::time::Duration::from_secs(60),
                start,
            )
            .unwrap();
        assert_eq!(grants.revoke_worker_generation(7), 1);
        assert_eq!(
            grants.validate_at(live, UtilityGrantKind::SharedMemoryRead, 2, 7, 0, 1, start,),
            Err(UtilityGrantError::Unknown)
        );
    }
}
