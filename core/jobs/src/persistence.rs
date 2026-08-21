//! Versioned append-only job queue persistence. [ADR-009, ADR-021]

use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

use crate::{GraphError, JobGraph, JobId, JobPriority, JobSpec};

const SNAPSHOT_MAGIC: &[u8; 8] = b"PDFJOBS\0";
const FRAME_MAGIC: &[u8; 8] = b"JOBSFRM\0";
const FORMAT_VERSION: u32 = 2;
const MAX_FILE_BYTES: usize = 16 * 1024 * 1024;
const MAX_SNAPSHOT_BYTES: usize = 4 * 1024 * 1024;
const MAX_JOBS: usize = 100_000;
const MAX_OPERATION_BYTES: usize = 64 * 1024;
const MAX_DEPENDENCIES: usize = 100_000;

/// Lifecycle state stored for restart recovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersistedJobState {
    /// Waiting for dependencies or capacity.
    Pending,
    /// Was executing when the snapshot was written; restores as pending.
    Running,
    /// Finished successfully.
    Completed,
    /// Finished with a failure.
    Failed,
    /// Cooperatively cancelled.
    Cancelled,
}

/// Validated persisted queue state.
#[derive(Debug, Clone)]
pub struct JobSnapshot {
    graph: JobGraph,
    states: HashMap<JobId, PersistedJobState>,
}

impl JobSnapshot {
    /// Create a snapshot with exactly one state for every graph job.
    pub fn new(
        graph: JobGraph,
        states: HashMap<JobId, PersistedJobState>,
    ) -> Result<Self, PersistenceError> {
        if states.len() != graph.jobs().len()
            || graph.jobs().iter().any(|job| !states.contains_key(&job.id))
        {
            return Err(PersistenceError::Malformed(
                "states do not match graph jobs",
            ));
        }
        Ok(Self { graph, states })
    }

    /// Persisted graph.
    pub fn graph(&self) -> &JobGraph {
        &self.graph
    }

    /// State for a job identifier.
    pub fn state(&self, job: JobId) -> Option<PersistedJobState> {
        self.states.get(&job).copied()
    }

    pub(crate) fn into_parts(self) -> (JobGraph, HashMap<JobId, PersistedJobState>) {
        (self.graph, self.states)
    }
}

/// Job persistence failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PersistenceError {
    /// Filesystem operation failed.
    Io(String),
    /// Snapshot version is newer or otherwise unsupported.
    UnsupportedVersion(u32),
    /// A complete frame contains invalid data.
    Malformed(&'static str),
    /// A declared or actual bound was exceeded.
    LimitExceeded(&'static str),
    /// Reconstructed graph is invalid.
    Graph(GraphError),
}

impl From<std::io::Error> for PersistenceError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

/// Encode one bounded versioned snapshot payload.
pub fn encode_snapshot(snapshot: &JobSnapshot) -> Result<Vec<u8>, PersistenceError> {
    let jobs = snapshot.graph.jobs();
    if jobs.len() > MAX_JOBS {
        return Err(PersistenceError::LimitExceeded("job count"));
    }
    let mut output = Vec::new();
    output.extend_from_slice(SNAPSHOT_MAGIC);
    put_u32(&mut output, FORMAT_VERSION);
    put_u32(&mut output, jobs.len() as u32);
    for job in jobs {
        if job.operation.len() > MAX_OPERATION_BYTES {
            return Err(PersistenceError::LimitExceeded("operation length"));
        }
        if job.dependencies.len() > MAX_DEPENDENCIES {
            return Err(PersistenceError::LimitExceeded("dependency count"));
        }
        put_u64(&mut output, job.id);
        output.push(priority_tag(job.priority));
        output.push(state_tag(snapshot.states[&job.id]));
        output.push(u8::from(job.idempotent));
        put_u32(&mut output, job.operation.len() as u32);
        output.extend_from_slice(job.operation.as_bytes());
        put_u32(&mut output, job.dependencies.len() as u32);
        for dependency in &job.dependencies {
            put_u64(&mut output, *dependency);
        }
        if output.len() > MAX_SNAPSHOT_BYTES {
            return Err(PersistenceError::LimitExceeded("snapshot size"));
        }
    }
    Ok(output)
}

/// Decode and validate one snapshot payload.
pub fn decode_snapshot(bytes: &[u8]) -> Result<JobSnapshot, PersistenceError> {
    if bytes.len() > MAX_SNAPSHOT_BYTES {
        return Err(PersistenceError::LimitExceeded("snapshot size"));
    }
    let mut cursor = Cursor::new(bytes);
    if cursor.take(8)? != SNAPSHOT_MAGIC {
        return Err(PersistenceError::Malformed("snapshot magic"));
    }
    let version = cursor.u32()?;
    if !matches!(version, 1 | FORMAT_VERSION) {
        return Err(PersistenceError::UnsupportedVersion(version));
    }
    let count = cursor.u32()? as usize;
    if count > MAX_JOBS {
        return Err(PersistenceError::LimitExceeded("job count"));
    }
    let mut jobs = Vec::with_capacity(count);
    let mut states = HashMap::with_capacity(count);
    for _ in 0..count {
        let id = cursor.u64()?;
        let priority = decode_priority(cursor.byte()?)?;
        let state = decode_state(cursor.byte()?)?;
        let idempotent = if version >= 2 {
            match cursor.byte()? {
                0 => false,
                1 => true,
                _ => return Err(PersistenceError::Malformed("idempotency tag")),
            }
        } else {
            false
        };
        let operation_len = cursor.u32()? as usize;
        if operation_len > MAX_OPERATION_BYTES {
            return Err(PersistenceError::LimitExceeded("operation length"));
        }
        let operation = std::str::from_utf8(cursor.take(operation_len)?)
            .map_err(|_| PersistenceError::Malformed("operation UTF-8"))?
            .to_owned();
        let dependency_count = cursor.u32()? as usize;
        if dependency_count > MAX_DEPENDENCIES {
            return Err(PersistenceError::LimitExceeded("dependency count"));
        }
        let mut dependencies = Vec::with_capacity(dependency_count);
        for _ in 0..dependency_count {
            dependencies.push(cursor.u64()?);
        }
        jobs.push(JobSpec {
            id,
            operation,
            priority,
            dependencies,
            idempotent,
        });
        if states.insert(id, normalize_state(state)).is_some() {
            return Err(PersistenceError::Malformed("duplicate job state"));
        }
    }
    if !cursor.is_empty() {
        return Err(PersistenceError::Malformed("trailing snapshot bytes"));
    }
    let graph = JobGraph::new(jobs).map_err(PersistenceError::Graph)?;
    JobSnapshot::new(graph, states)
}

/// Append and durably flush one committed snapshot frame.
pub fn append_snapshot(path: &Path, snapshot: &JobSnapshot) -> Result<(), PersistenceError> {
    let payload = encode_snapshot(snapshot)?;
    let length = payload.len() as u32;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(FRAME_MAGIC)?;
    file.write_all(&length.to_le_bytes())?;
    file.write_all(&payload)?;
    file.write_all(&length.to_le_bytes())?;
    file.sync_data()?;
    Ok(())
}

/// Load the newest complete valid snapshot, ignoring only a torn trailing frame.
pub fn load_latest(path: &Path) -> Result<Option<JobSnapshot>, PersistenceError> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if bytes.len() > MAX_FILE_BYTES {
        return Err(PersistenceError::LimitExceeded("log size"));
    }
    let mut offset = 0usize;
    let mut latest = None;
    while offset < bytes.len() {
        let remaining = &bytes[offset..];
        if remaining.len() < 12 {
            break;
        }
        if &remaining[..8] != FRAME_MAGIC {
            return Err(PersistenceError::Malformed("frame magic"));
        }
        let length = u32::from_le_bytes(remaining[8..12].try_into().expect("four bytes")) as usize;
        if length > MAX_SNAPSHOT_BYTES {
            return Err(PersistenceError::LimitExceeded("snapshot size"));
        }
        let frame_len = 12usize
            .checked_add(length)
            .and_then(|value| value.checked_add(4))
            .ok_or(PersistenceError::LimitExceeded("frame size"))?;
        if remaining.len() < frame_len {
            break;
        }
        let committed = u32::from_le_bytes(
            remaining[12 + length..frame_len]
                .try_into()
                .expect("four bytes"),
        ) as usize;
        if committed != length {
            return Err(PersistenceError::Malformed("frame commit length"));
        }
        latest = Some(decode_snapshot(&remaining[12..12 + length])?);
        offset += frame_len;
    }
    Ok(latest)
}

fn normalize_state(state: PersistedJobState) -> PersistedJobState {
    match state {
        PersistedJobState::Running => PersistedJobState::Pending,
        other => other,
    }
}

fn priority_tag(priority: JobPriority) -> u8 {
    match priority {
        JobPriority::Maintenance => 0,
        JobPriority::UserInitiated => 1,
        JobPriority::InteractiveAdjacent => 2,
    }
}

fn decode_priority(tag: u8) -> Result<JobPriority, PersistenceError> {
    match tag {
        0 => Ok(JobPriority::Maintenance),
        1 => Ok(JobPriority::UserInitiated),
        2 => Ok(JobPriority::InteractiveAdjacent),
        _ => Err(PersistenceError::Malformed("priority tag")),
    }
}

fn state_tag(state: PersistedJobState) -> u8 {
    match state {
        PersistedJobState::Pending => 0,
        PersistedJobState::Running => 1,
        PersistedJobState::Completed => 2,
        PersistedJobState::Failed => 3,
        PersistedJobState::Cancelled => 4,
    }
}

fn decode_state(tag: u8) -> Result<PersistedJobState, PersistenceError> {
    match tag {
        0 => Ok(PersistedJobState::Pending),
        1 => Ok(PersistedJobState::Running),
        2 => Ok(PersistedJobState::Completed),
        3 => Ok(PersistedJobState::Failed),
        4 => Ok(PersistedJobState::Cancelled),
        _ => Err(PersistenceError::Malformed("state tag")),
    }
}

fn put_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn put_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_le_bytes());
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], PersistenceError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(PersistenceError::Malformed("length overflow"))?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(PersistenceError::Malformed("truncated snapshot"))?;
        self.offset = end;
        Ok(value)
    }

    fn byte(&mut self) -> Result<u8, PersistenceError> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, PersistenceError> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("four bytes"),
        ))
    }

    fn u64(&mut self) -> Result<u64, PersistenceError> {
        Ok(u64::from_le_bytes(
            self.take(8)?.try_into().expect("eight bytes"),
        ))
    }

    fn is_empty(&self) -> bool {
        self.offset == self.bytes.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{JobGraph, JobPriority, JobSpec};
    use std::collections::HashMap;
    use std::io::Write;

    fn snapshot(operation: &str, state: PersistedJobState) -> JobSnapshot {
        let graph = JobGraph::new(vec![
            JobSpec::new(1, operation, JobPriority::InteractiveAdjacent),
            JobSpec::new(2, "second", JobPriority::Maintenance).depends_on(1),
        ])
        .unwrap();
        JobSnapshot::new(
            graph,
            HashMap::from([(1, state), (2, PersistedJobState::Pending)]),
        )
        .unwrap()
    }

    #[test]
    fn snapshot_codec_round_trips_graph_and_states() {
        let mut original = snapshot("ocr page 1", PersistedJobState::Completed);
        original.graph.jobs[0].idempotent = true;
        let encoded = encode_snapshot(&original).unwrap();
        let decoded = decode_snapshot(&encoded).unwrap();
        assert_eq!(decoded.graph().jobs()[0].operation, "ocr page 1");
        assert_eq!(decoded.graph().jobs()[1].dependencies, vec![1]);
        assert_eq!(decoded.state(1), Some(PersistedJobState::Completed));
        assert!(decoded.graph().jobs()[0].idempotent);
    }

    #[test]
    fn version_one_jobs_restore_as_non_idempotent() {
        let mut encoded = Vec::new();
        encoded.extend_from_slice(SNAPSHOT_MAGIC);
        put_u32(&mut encoded, 1);
        put_u32(&mut encoded, 1);
        put_u64(&mut encoded, 7);
        encoded.push(priority_tag(JobPriority::Maintenance));
        encoded.push(state_tag(PersistedJobState::Pending));
        put_u32(&mut encoded, 3);
        encoded.extend_from_slice(b"ocr");
        put_u32(&mut encoded, 0);

        let decoded = decode_snapshot(&encoded).unwrap();
        assert!(!decoded.graph().jobs()[0].idempotent);
    }

    #[test]
    fn running_state_restores_as_pending() {
        let encoded = encode_snapshot(&snapshot("ocr", PersistedJobState::Running)).unwrap();
        let decoded = decode_snapshot(&encoded).unwrap();
        assert_eq!(decoded.state(1), Some(PersistedJobState::Pending));
    }

    #[test]
    fn codec_rejects_unknown_version() {
        let mut encoded = encode_snapshot(&snapshot("ocr", PersistedJobState::Pending)).unwrap();
        encoded[8..12].copy_from_slice(&99u32.to_le_bytes());
        assert_eq!(
            decode_snapshot(&encoded).unwrap_err(),
            PersistenceError::UnsupportedVersion(99)
        );
    }

    #[test]
    fn codec_enforces_operation_bound() {
        let operation = "x".repeat(MAX_OPERATION_BYTES + 1);
        assert_eq!(
            encode_snapshot(&snapshot(&operation, PersistedJobState::Pending)).unwrap_err(),
            PersistenceError::LimitExceeded("operation length")
        );
    }

    #[test]
    fn store_loads_latest_complete_snapshot_and_ignores_torn_tail() {
        let path = temp_path();
        append_snapshot(&path, &snapshot("first", PersistedJobState::Pending)).unwrap();
        append_snapshot(&path, &snapshot("latest", PersistedJobState::Completed)).unwrap();
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"JOBSFRM\0\x20\0\0")
            .unwrap();

        let loaded = load_latest(&path).unwrap().unwrap();
        assert_eq!(loaded.graph().jobs()[0].operation, "latest");
        std::fs::remove_file(path).ok();
    }

    fn temp_path() -> std::path::PathBuf {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        std::env::temp_dir().join(format!(
            "pdf-platform-jobs-{}-{}.log",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ))
    }
}
