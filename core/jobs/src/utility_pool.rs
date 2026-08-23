//! Fixed sandboxed utility-worker process pool. [ADR-008, ADR-009]

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use protocol::utility_jobs::{
    decode_event, encode_command, UtilityJobCommand, UtilityJobEvent, UtilityJobInput,
};
use sandbox::shmem::SharedRegion;
use sandbox::spawn::{
    spawn_utility_worker_with_attachments, spawn_utility_worker_with_shmem, WorkerChild,
};

use crate::{JobContext, JobRunError, JobSpec};

/// Default fixed shared-memory capacity for each utility process (16 MiB). [GR-7]
pub const DEFAULT_UTILITY_SHMEM_BYTES: usize = 16 * 1024 * 1024;

/// Utility pool construction failure.
#[derive(Debug)]
pub enum UtilityPoolError {
    /// A pool must contain at least one process.
    ZeroWorkers,
    /// A sandboxed worker could not be spawned.
    Spawn(std::io::Error),
    /// Requested slot does not exist.
    UnknownWorkerSlot(usize),
}

impl std::fmt::Display for UtilityPoolError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ZeroWorkers => write!(formatter, "utility pool requires at least one worker"),
            Self::Spawn(error) => write!(formatter, "utility worker spawn failed: {error}"),
            Self::UnknownWorkerSlot(slot) => {
                write!(formatter, "unknown utility worker slot: {slot}")
            }
        }
    }
}

impl std::error::Error for UtilityPoolError {}

/// Fixed set of sandbox-spawned utility worker processes.
pub struct UtilityPool {
    worker_exe: PathBuf,
    workers: Vec<Mutex<UtilityWorkerSlot>>,
    next_worker: AtomicUsize,
    next_correlation: AtomicU64,
    generations: Vec<AtomicU64>,
    replacement_hook: Arc<dyn Fn(UtilityWorkerIdentity) + Send + Sync>,
    response_timeout: Duration,
}

struct UtilityWorkerSlot {
    child: WorkerChild,
    shared_memory: SharedRegion,
}

/// Stable worker slot plus incarnation counter used to scope capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UtilityWorkerIdentity {
    /// Fixed pool slot.
    pub slot: usize,
    /// Incremented whenever the process in this slot is replaced.
    pub generation: u64,
}

/// Z0 preparation view for the selected and locked utility worker.
pub struct UtilityWorkerPreparation<'a> {
    /// Exact worker incarnation that will receive the request.
    pub identity: UtilityWorkerIdentity,
    /// Fixed-capacity shared-memory region inherited by that worker.
    pub shared_memory: &'a mut [u8],
}

impl UtilityPool {
    /// Spawn a fixed-size utility worker pool.
    pub fn new(
        worker_exe: impl AsRef<Path>,
        worker_count: usize,
    ) -> Result<Self, UtilityPoolError> {
        Self::new_with_replacement_hook(worker_exe, worker_count, |_| {})
    }

    /// Spawn a pool and observe invalidated worker generations before replacement.
    pub fn new_with_replacement_hook<F>(
        worker_exe: impl AsRef<Path>,
        worker_count: usize,
        replacement_hook: F,
    ) -> Result<Self, UtilityPoolError>
    where
        F: Fn(UtilityWorkerIdentity) + Send + Sync + 'static,
    {
        Self::new_with_capacity_and_hook(
            worker_exe,
            worker_count,
            DEFAULT_UTILITY_SHMEM_BYTES,
            replacement_hook,
        )
    }

    /// Spawn a pool with an explicit per-worker shared-memory bound.
    pub fn new_with_capacity_and_hook<F>(
        worker_exe: impl AsRef<Path>,
        worker_count: usize,
        shared_memory_bytes: usize,
        replacement_hook: F,
    ) -> Result<Self, UtilityPoolError>
    where
        F: Fn(UtilityWorkerIdentity) + Send + Sync + 'static,
    {
        if worker_count == 0 {
            return Err(UtilityPoolError::ZeroWorkers);
        }
        if shared_memory_bytes == 0 {
            return Err(UtilityPoolError::Spawn(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "utility shared-memory capacity must be non-zero",
            )));
        }
        let worker_exe = worker_exe.as_ref().to_path_buf();
        let mut workers = Vec::with_capacity(worker_count);
        for _ in 0..worker_count {
            let shared_memory =
                SharedRegion::create(shared_memory_bytes).map_err(UtilityPoolError::Spawn)?;
            match spawn_utility_worker_with_shmem(&worker_exe, shared_memory.file()) {
                Ok(child) => workers.push(Mutex::new(UtilityWorkerSlot {
                    child,
                    shared_memory,
                })),
                Err(error) => {
                    stop_workers(&mut workers);
                    return Err(UtilityPoolError::Spawn(error));
                }
            }
        }
        Ok(Self {
            worker_exe,
            workers,
            next_worker: AtomicUsize::new(0),
            next_correlation: AtomicU64::new(1),
            generations: (0..worker_count).map(|_| AtomicU64::new(0)).collect(),
            replacement_hook: Arc::new(replacement_hook),
            response_timeout: Duration::from_secs(30),
        })
    }

    /// Number of fixed worker slots.
    pub fn worker_count(&self) -> usize {
        self.workers.len()
    }

    /// Read a worker's shared-memory region.
    ///
    /// The region is where a utility job puts its *output* — a thumbnail's
    /// pixels never travel back through IPC — and nothing could observe it, so
    /// a job that returned a well-formed descriptor and drew nothing looked
    /// exactly like one that worked. Read-only and borrowed for the call, so a
    /// caller cannot retain or mutate the region. [FR-VIEW-3, GR-8, PRIN-6]
    pub fn inspect_shared_memory<T>(
        &self,
        worker_index: usize,
        read: impl FnOnce(&[u8]) -> T,
    ) -> Option<T> {
        let worker = self.workers.get(worker_index)?;
        let guard = worker.lock().ok()?;
        Some(read(guard.shared_memory.as_slice()))
    }

    /// Replace one worker and invalidate capabilities bound to its old generation.
    pub fn restart_worker(&self, slot: usize) -> Result<(), UtilityPoolError> {
        let worker = self
            .workers
            .get(slot)
            .ok_or(UtilityPoolError::UnknownWorkerSlot(slot))?;
        let mut worker = worker
            .lock()
            .map_err(|_| UtilityPoolError::Spawn(std::io::Error::other("worker lock poisoned")))?;
        let identity = UtilityWorkerIdentity {
            slot,
            generation: self.generations[slot].fetch_add(1, Ordering::AcqRel),
        };
        (self.replacement_hook)(identity);
        replace_worker(&self.worker_exe, &mut worker).map_err(UtilityPoolError::Spawn)
    }

    /// Execute one declarative job through a utility process.
    pub fn execute(&self, spec: JobSpec, context: JobContext) -> Result<(), JobRunError> {
        self.execute_prepared_result(spec, context, |_| Ok(Vec::new()))
            .map(|_| ())
    }

    /// Select and lock a worker, then prepare inputs scoped to that exact incarnation.
    pub fn execute_prepared<P>(
        &self,
        spec: JobSpec,
        context: JobContext,
        prepare: P,
    ) -> Result<(), JobRunError>
    where
        P: FnOnce(UtilityWorkerPreparation<'_>) -> Result<Vec<UtilityJobInput>, JobRunError>,
    {
        self.execute_prepared_result(spec, context, prepare)
            .map(|_| ())
    }

    /// Select and lock a worker, prepare its inputs, and return bounded result bytes.
    pub fn execute_prepared_result<P>(
        &self,
        spec: JobSpec,
        context: JobContext,
        prepare: P,
    ) -> Result<Vec<u8>, JobRunError>
    where
        P: FnOnce(UtilityWorkerPreparation<'_>) -> Result<Vec<UtilityJobInput>, JobRunError>,
    {
        self.execute_prepared_result_inner(spec, context, None, prepare)
    }

    /// Run one job in a fresh incarnation holding a brokered document handle.
    ///
    /// The slot is returned to a document-free worker after the terminal event.
    pub fn execute_prepared_with_document<P>(
        &self,
        spec: JobSpec,
        context: JobContext,
        document: &File,
        password: Option<&str>,
        prepare: P,
    ) -> Result<Vec<u8>, JobRunError>
    where
        P: FnOnce(UtilityWorkerPreparation<'_>) -> Result<Vec<UtilityJobInput>, JobRunError>,
    {
        self.execute_prepared_result_inner(spec, context, Some((document, password)), prepare)
    }

    fn execute_prepared_result_inner<P>(
        &self,
        spec: JobSpec,
        context: JobContext,
        document: Option<(&File, Option<&str>)>,
        prepare: P,
    ) -> Result<Vec<u8>, JobRunError>
    where
        P: FnOnce(UtilityWorkerPreparation<'_>) -> Result<Vec<UtilityJobInput>, JobRunError>,
    {
        if context.is_cancelled() {
            return Err(JobRunError::Execution(
                "job cancelled before dispatch".into(),
            ));
        }
        let index = self.next_worker.fetch_add(1, Ordering::Relaxed) % self.workers.len();
        let correlation_id = self.next_correlation.fetch_add(1, Ordering::Relaxed);
        let mut worker = self.workers[index]
            .lock()
            .map_err(|_| JobRunError::WorkerCrashed("utility worker lock poisoned".into()))?;
        if let Some((document, password)) = document {
            let old_identity = UtilityWorkerIdentity {
                slot: index,
                generation: self.generations[index].fetch_add(1, Ordering::AcqRel),
            };
            (self.replacement_hook)(old_identity);
            if let Err(error) =
                replace_worker_with_document(&self.worker_exe, &mut worker, document, password)
            {
                let _ = replace_worker(&self.worker_exe, &mut worker);
                return Err(JobRunError::WorkerCrashed(format!(
                    "utility document attachment failed: {error}"
                )));
            }
        }
        let identity = UtilityWorkerIdentity {
            slot: index,
            generation: self.generations[index].load(Ordering::Acquire),
        };
        let inputs = prepare(UtilityWorkerPreparation {
            identity,
            shared_memory: worker.shared_memory.as_mut_slice(),
        })?;
        let request = UtilityJobCommand {
            correlation_id,
            job_id: spec.id,
            operation: spec.operation,
            inputs,
        };
        let frame = encode_command(&request).map_err(|error| {
            JobRunError::Execution(format!("job request encoding failed: {error:?}"))
        })?;
        let result = worker
            .child
            .transport
            .send(&frame)
            .and_then(|()| worker.child.transport.recv_timeout(self.response_timeout))
            .map_err(|error| format!("utility transport failed: {error}"))
            .and_then(|bytes| {
                decode_event(&bytes).map_err(|error| format!("invalid utility response: {error:?}"))
            });
        let (job_result, transport_broken) = match result {
            Ok(UtilityJobEvent::Completed {
                correlation_id: cid,
                job_id,
                output,
            }) if cid == correlation_id && job_id == spec.id => (Ok(output), false),
            Ok(UtilityJobEvent::Failed {
                correlation_id: cid,
                job_id,
                message,
            }) if cid == correlation_id && job_id == spec.id => {
                (Err(JobRunError::Execution(message)), false)
            }
            Ok(_) => (
                Err(JobRunError::WorkerCrashed(
                    "mismatched utility response".into(),
                )),
                true,
            ),
            Err(message) => (Err(JobRunError::WorkerCrashed(message)), true),
        };
        if document.is_some() || transport_broken {
            (self.replacement_hook)(identity);
            self.generations[index].fetch_add(1, Ordering::AcqRel);
            if let Err(error) = replace_worker(&self.worker_exe, &mut worker) {
                return Err(JobRunError::WorkerCrashed(format!(
                    "utility worker scrub failed: {error}"
                )));
            }
        }
        job_result
    }
}

impl Drop for UtilityPool {
    fn drop(&mut self) {
        stop_workers(&mut self.workers);
    }
}

fn replace_worker(worker_exe: &Path, worker: &mut UtilityWorkerSlot) -> std::io::Result<()> {
    let _ = worker.child.child.kill();
    let _ = worker.child.child.wait();
    worker.child = spawn_utility_worker_with_shmem(worker_exe, worker.shared_memory.file())?;
    Ok(())
}

fn replace_worker_with_document(
    worker_exe: &Path,
    worker: &mut UtilityWorkerSlot,
    document: &File,
    password: Option<&str>,
) -> std::io::Result<()> {
    let _ = worker.child.child.kill();
    let _ = worker.child.child.wait();
    worker.child = spawn_utility_worker_with_attachments(
        worker_exe,
        document,
        worker.shared_memory.file(),
        password,
    )?;
    Ok(())
}

fn stop_workers(workers: &mut [Mutex<UtilityWorkerSlot>]) {
    for worker in workers {
        if let Ok(worker) = worker.get_mut() {
            let _ = worker.child.child.kill();
            let _ = worker.child.child.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pool_rejects_zero_workers_without_spawning() {
        assert!(matches!(
            UtilityPool::new("missing-worker", 0),
            Err(UtilityPoolError::ZeroWorkers)
        ));
    }
}
