//! Fixed sandboxed utility-worker process pool. [ADR-008, ADR-009]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use protocol::utility_jobs::{decode_event, encode_command, UtilityJobCommand, UtilityJobEvent};
use sandbox::spawn::{spawn_utility_worker, WorkerChild};

use crate::{JobContext, JobRunError, JobSpec};

/// Utility pool construction failure.
#[derive(Debug)]
pub enum UtilityPoolError {
    /// A pool must contain at least one process.
    ZeroWorkers,
    /// A sandboxed worker could not be spawned.
    Spawn(std::io::Error),
}

impl std::fmt::Display for UtilityPoolError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ZeroWorkers => write!(formatter, "utility pool requires at least one worker"),
            Self::Spawn(error) => write!(formatter, "utility worker spawn failed: {error}"),
        }
    }
}

impl std::error::Error for UtilityPoolError {}

/// Fixed set of sandbox-spawned utility worker processes.
pub struct UtilityPool {
    worker_exe: PathBuf,
    workers: Vec<Mutex<WorkerChild>>,
    next_worker: AtomicUsize,
    next_correlation: AtomicU64,
    response_timeout: Duration,
}

impl UtilityPool {
    /// Spawn a fixed-size utility worker pool.
    pub fn new(
        worker_exe: impl AsRef<Path>,
        worker_count: usize,
    ) -> Result<Self, UtilityPoolError> {
        if worker_count == 0 {
            return Err(UtilityPoolError::ZeroWorkers);
        }
        let worker_exe = worker_exe.as_ref().to_path_buf();
        let mut workers = Vec::with_capacity(worker_count);
        for _ in 0..worker_count {
            match spawn_utility_worker(&worker_exe) {
                Ok(worker) => workers.push(Mutex::new(worker)),
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
            response_timeout: Duration::from_secs(30),
        })
    }

    /// Number of fixed worker slots.
    pub fn worker_count(&self) -> usize {
        self.workers.len()
    }

    /// Execute one declarative job through a utility process.
    pub fn execute(&self, spec: JobSpec, context: JobContext) -> Result<(), JobRunError> {
        if context.is_cancelled() {
            return Err(JobRunError::Execution(
                "job cancelled before dispatch".into(),
            ));
        }
        let index = self.next_worker.fetch_add(1, Ordering::Relaxed) % self.workers.len();
        let correlation_id = self.next_correlation.fetch_add(1, Ordering::Relaxed);
        let request = UtilityJobCommand {
            correlation_id,
            job_id: spec.id,
            operation: spec.operation,
        };
        let frame = encode_command(&request).map_err(|error| {
            JobRunError::Execution(format!("job request encoding failed: {error:?}"))
        })?;
        let mut worker = self.workers[index]
            .lock()
            .map_err(|_| JobRunError::WorkerCrashed("utility worker lock poisoned".into()))?;
        let result = worker
            .transport
            .send(&frame)
            .and_then(|()| worker.transport.recv_timeout(self.response_timeout))
            .map_err(|error| format!("utility transport failed: {error}"))
            .and_then(|bytes| {
                decode_event(&bytes).map_err(|error| format!("invalid utility response: {error:?}"))
            });
        match result {
            Ok(UtilityJobEvent::Completed {
                correlation_id: cid,
                job_id,
            }) if cid == correlation_id && job_id == spec.id => Ok(()),
            Ok(UtilityJobEvent::Failed {
                correlation_id: cid,
                job_id,
                message,
            }) if cid == correlation_id && job_id == spec.id => {
                Err(JobRunError::Execution(message))
            }
            Ok(_) => {
                replace_worker(&self.worker_exe, &mut worker);
                Err(JobRunError::WorkerCrashed(
                    "mismatched utility response".into(),
                ))
            }
            Err(message) => {
                replace_worker(&self.worker_exe, &mut worker);
                Err(JobRunError::WorkerCrashed(message))
            }
        }
    }
}

impl Drop for UtilityPool {
    fn drop(&mut self) {
        stop_workers(&mut self.workers);
    }
}

fn replace_worker(worker_exe: &Path, worker: &mut WorkerChild) {
    let _ = worker.child.kill();
    let _ = worker.child.wait();
    if let Ok(replacement) = spawn_utility_worker(worker_exe) {
        *worker = replacement;
    }
}

fn stop_workers(workers: &mut [Mutex<WorkerChild>]) {
    for worker in workers {
        if let Ok(worker) = worker.get_mut() {
            let _ = worker.child.kill();
            let _ = worker.child.wait();
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
