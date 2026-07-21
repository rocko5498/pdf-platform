//! Bounded, cancellable declarative job DAG scheduler. [ADR-009]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod persistence;

use std::cmp::Ordering as CmpOrdering;
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc::{self, Receiver, RecvTimeoutError, Sender},
    Arc, Mutex,
};
use std::thread::JoinHandle;
use std::time::Duration;
use std::{path::Path, path::PathBuf};

use persistence::{JobSnapshot, PersistedJobState, PersistenceError};

/// Stable identifier for one job within a graph.
pub type JobId = u64;

/// Scheduling priority classes from ADR-009.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum JobPriority {
    /// Indexing and other maintenance with no user waiting.
    Maintenance,
    /// Explicitly requested batch work.
    UserInitiated,
    /// Work adjacent to an interactive action.
    InteractiveAdjacent,
}

/// Declarative metadata for one schedulable operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobSpec {
    /// Stable job identifier.
    pub id: JobId,
    /// Operation name understood by the executor.
    pub operation: String,
    /// Scheduling priority.
    pub priority: JobPriority,
    /// Jobs that must complete first.
    pub dependencies: Vec<JobId>,
    /// Whether this operation is safe to repeat after utility-worker loss.
    pub idempotent: bool,
}

impl JobSpec {
    /// Create a job with no dependencies.
    pub fn new(id: JobId, operation: impl Into<String>, priority: JobPriority) -> Self {
        Self {
            id,
            operation: operation.into(),
            priority,
            dependencies: Vec::new(),
            idempotent: false,
        }
    }

    /// Add a dependency.
    pub fn depends_on(mut self, dependency: JobId) -> Self {
        self.dependencies.push(dependency);
        self
    }

    /// Declare that the operation may be retried after worker loss.
    pub fn idempotent(mut self) -> Self {
        self.idempotent = true;
        self
    }
}

/// Invalid declarative job graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphError {
    /// Two jobs use the same identifier.
    DuplicateJob(JobId),
    /// A dependency is not present in the graph.
    MissingDependency {
        /// Job containing the invalid dependency.
        job: JobId,
        /// Dependency identifier absent from the graph.
        dependency: JobId,
    },
    /// Dependencies contain a cycle.
    Cycle,
}

/// Validated acyclic set of jobs.
#[derive(Debug, Clone)]
pub struct JobGraph {
    jobs: Vec<JobSpec>,
}

impl JobGraph {
    /// Validate and construct a job graph.
    pub fn new(jobs: Vec<JobSpec>) -> Result<Self, GraphError> {
        let mut indexes = HashMap::with_capacity(jobs.len());
        for (index, job) in jobs.iter().enumerate() {
            if indexes.insert(job.id, index).is_some() {
                return Err(GraphError::DuplicateJob(job.id));
            }
        }
        for job in &jobs {
            for dependency in &job.dependencies {
                if !indexes.contains_key(dependency) {
                    return Err(GraphError::MissingDependency {
                        job: job.id,
                        dependency: *dependency,
                    });
                }
            }
        }

        let mut indegree = HashMap::with_capacity(jobs.len());
        let mut dependants: HashMap<JobId, Vec<JobId>> = HashMap::new();
        for job in &jobs {
            indegree.insert(job.id, job.dependencies.len());
            for dependency in &job.dependencies {
                dependants.entry(*dependency).or_default().push(job.id);
            }
        }
        let mut ready: VecDeque<JobId> = indegree
            .iter()
            .filter_map(|(&id, &degree)| (degree == 0).then_some(id))
            .collect();
        let mut visited = 0;
        while let Some(id) = ready.pop_front() {
            visited += 1;
            for dependant in dependants.get(&id).into_iter().flatten() {
                let degree = indegree.get_mut(dependant).expect("validated dependant");
                *degree -= 1;
                if *degree == 0 {
                    ready.push_back(*dependant);
                }
            }
        }
        if visited != jobs.len() {
            return Err(GraphError::Cycle);
        }
        Ok(Self { jobs })
    }

    /// Jobs in submission order.
    pub fn jobs(&self) -> &[JobSpec] {
        &self.jobs
    }

    fn into_jobs(self) -> Vec<JobSpec> {
        self.jobs
    }
}

/// Cooperatively shared cancellation flag.
#[derive(Debug, Clone, Default)]
pub struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    /// Create a non-cancelled token.
    pub fn new() -> Self {
        Self::default()
    }

    /// Request cancellation.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    /// Whether cancellation was requested.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// Structured scheduler lifecycle event.
#[derive(Debug, Clone, PartialEq)]
pub enum JobEvent {
    /// Job accepted into the bounded queue.
    Queued {
        /// Job identifier.
        job: JobId,
    },
    /// Executor started the job.
    Started {
        /// Job identifier.
        job: JobId,
    },
    /// Executor-reported progress from zero through one.
    Progress {
        /// Job identifier.
        job: JobId,
        /// Completed fraction, clamped to zero through one.
        fraction: f32,
    },
    /// Job completed successfully.
    Completed {
        /// Job identifier.
        job: JobId,
    },
    /// Job failed or could not run because a dependency failed.
    Failed {
        /// Job identifier.
        job: JobId,
        /// Human-readable failure detail.
        message: String,
    },
    /// Cooperative cancellation reached a terminal state.
    Cancelled {
        /// Job identifier.
        job: JobId,
    },
    /// A durable queue checkpoint could not be written.
    PersistenceFailed {
        /// Human-readable persistence failure detail.
        message: String,
    },
    /// The utility worker executing a job exited unexpectedly.
    WorkerCrashed {
        /// Job identifier.
        job: JobId,
        /// Whether the scheduler will retry the job.
        will_retry: bool,
        /// Human-readable worker failure detail.
        message: String,
    },
}

/// Typed result failure from a utility-job executor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobRunError {
    /// The operation ran but failed.
    Execution(String),
    /// The utility worker exited or became unusable during the operation.
    WorkerCrashed(String),
}

/// Context supplied to a job executor.
#[derive(Clone)]
pub struct JobContext {
    job: JobId,
    token: CancellationToken,
    events: Sender<JobEvent>,
}

impl JobContext {
    /// Whether the executor should unwind at its next safe point.
    pub fn is_cancelled(&self) -> bool {
        self.token.is_cancelled()
    }

    /// Emit clamped progress for this job.
    pub fn report_progress(&self, fraction: f32) {
        let _ = self.events.send(JobEvent::Progress {
            job: self.job,
            fraction: fraction.clamp(0.0, 1.0),
        });
    }
}

/// Scheduler construction error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchedulerError {
    /// Worker count or queue capacity was zero.
    ZeroCapacity,
    /// A worker or scheduler thread could not be created.
    ThreadSpawn(String),
    /// Durable queue state could not be loaded.
    Persistence(PersistenceError),
    /// Restored unfinished work exceeds the configured live-job bound.
    RestoreExceedsCapacity,
}

/// Job submission error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubmitError {
    /// The declared queue bound would be exceeded.
    QueueFull,
    /// A live job already uses this identifier.
    DuplicateJob(JobId),
    /// Scheduler has shut down.
    Closed,
}

type Executor = dyn Fn(JobSpec, JobContext) -> Result<(), JobRunError> + Send + Sync + 'static;

enum Control {
    Submit(JobGraph),
    Shutdown,
}

struct Work {
    spec: JobSpec,
    token: CancellationToken,
}

struct WorkDone {
    job: JobId,
    result: Result<(), JobRunError>,
    cancelled: bool,
}

#[derive(Eq)]
struct Ready {
    priority: JobPriority,
    sequence: usize,
    job: JobId,
}

impl Ord for Ready {
    fn cmp(&self, other: &Self) -> CmpOrdering {
        self.priority
            .cmp(&other.priority)
            .then_with(|| other.sequence.cmp(&self.sequence))
    }
}

impl PartialOrd for Ready {
    fn partial_cmp(&self, other: &Self) -> Option<CmpOrdering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Ready {
    fn eq(&self, other: &Self) -> bool {
        self.job == other.job
    }
}

struct Pending {
    spec: JobSpec,
    remaining: usize,
    blocked: bool,
    crash_retries: u8,
}

/// Bounded scheduler backed by named standard-library worker threads.
pub struct JobScheduler {
    control: Sender<Control>,
    events: Mutex<Receiver<JobEvent>>,
    tokens: Arc<Mutex<HashMap<JobId, CancellationToken>>>,
    persistent_ids: Option<Arc<Mutex<HashSet<JobId>>>>,
    reservations: Arc<AtomicUsize>,
    capacity: usize,
    scheduler_thread: Option<JoinHandle<()>>,
}

impl JobScheduler {
    /// Create a scheduler with a fixed worker count and bounded live-job capacity.
    pub fn new<F>(worker_count: usize, capacity: usize, executor: F) -> Result<Self, SchedulerError>
    where
        F: Fn(JobSpec, JobContext) -> Result<(), String> + Send + Sync + 'static,
    {
        Self::create(
            worker_count,
            capacity,
            move |spec, context| executor(spec, context).map_err(JobRunError::Execution),
            None,
            None,
        )
    }

    /// Create a scheduler whose executor can distinguish worker loss from operation failure.
    pub fn new_typed<F>(
        worker_count: usize,
        capacity: usize,
        executor: F,
    ) -> Result<Self, SchedulerError>
    where
        F: Fn(JobSpec, JobContext) -> Result<(), JobRunError> + Send + Sync + 'static,
    {
        Self::create(worker_count, capacity, executor, None, None)
    }

    /// Create a scheduler that checkpoints transitions and restores unfinished jobs.
    pub fn new_persistent<F>(
        worker_count: usize,
        capacity: usize,
        path: impl AsRef<Path>,
        executor: F,
    ) -> Result<Self, SchedulerError>
    where
        F: Fn(JobSpec, JobContext) -> Result<(), String> + Send + Sync + 'static,
    {
        let (path, restored) = load_persistent(path.as_ref(), capacity)?;
        Self::create(
            worker_count,
            capacity,
            move |spec, context| executor(spec, context).map_err(JobRunError::Execution),
            Some(path),
            restored,
        )
    }

    /// Create a persistent scheduler with typed utility-worker failures.
    pub fn new_persistent_typed<F>(
        worker_count: usize,
        capacity: usize,
        path: impl AsRef<Path>,
        executor: F,
    ) -> Result<Self, SchedulerError>
    where
        F: Fn(JobSpec, JobContext) -> Result<(), JobRunError> + Send + Sync + 'static,
    {
        let (path, restored) = load_persistent(path.as_ref(), capacity)?;
        Self::create(worker_count, capacity, executor, Some(path), restored)
    }

    fn create<F>(
        worker_count: usize,
        capacity: usize,
        executor: F,
        persistence_path: Option<PathBuf>,
        restored: Option<JobSnapshot>,
    ) -> Result<Self, SchedulerError>
    where
        F: Fn(JobSpec, JobContext) -> Result<(), JobRunError> + Send + Sync + 'static,
    {
        if worker_count == 0 || capacity == 0 {
            return Err(SchedulerError::ZeroCapacity);
        }
        let executor: Arc<Executor> = Arc::new(executor);
        let (control_tx, control_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let (init_tx, init_rx) = mpsc::channel();
        let tokens = Arc::new(Mutex::new(HashMap::new()));
        let restored_live = restored
            .as_ref()
            .map(|snapshot| {
                snapshot
                    .graph()
                    .jobs()
                    .iter()
                    .filter(|job| snapshot.state(job.id) == Some(PersistedJobState::Pending))
                    .map(|job| job.id)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let persistent_ids = persistence_path.as_ref().map(|_| {
            Arc::new(Mutex::new(
                restored
                    .as_ref()
                    .map(|snapshot| snapshot.graph().jobs().iter().map(|job| job.id).collect())
                    .unwrap_or_default(),
            ))
        });
        {
            let mut map = tokens.lock().expect("job token lock poisoned");
            for id in &restored_live {
                map.insert(*id, CancellationToken::new());
            }
        }
        let reservations = Arc::new(AtomicUsize::new(restored_live.len()));
        let thread_tokens = tokens.clone();
        let thread_reservations = reservations.clone();
        let scheduler_thread = std::thread::Builder::new()
            .name("pdf-job-scheduler".into())
            .spawn(move || {
                scheduler_loop(
                    worker_count,
                    executor,
                    control_rx,
                    event_tx,
                    thread_tokens,
                    thread_reservations,
                    init_tx,
                    persistence_path,
                    restored,
                )
            })
            .map_err(|error| SchedulerError::ThreadSpawn(error.to_string()))?;
        match init_rx.recv() {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                let _ = scheduler_thread.join();
                return Err(SchedulerError::ThreadSpawn(error));
            }
            Err(error) => {
                let _ = scheduler_thread.join();
                return Err(SchedulerError::ThreadSpawn(error.to_string()));
            }
        }
        Ok(Self {
            control: control_tx,
            events: Mutex::new(event_rx),
            tokens,
            persistent_ids,
            reservations,
            capacity,
            scheduler_thread: Some(scheduler_thread),
        })
    }

    /// Submit a validated graph without exceeding the configured live-job bound.
    pub fn submit(&self, graph: JobGraph) -> Result<(), SubmitError> {
        let jobs = graph.jobs();
        let job_count = jobs.len();
        let job_ids = jobs.iter().map(|job| job.id).collect::<Vec<_>>();
        {
            if let Some(ids) = &self.persistent_ids {
                let ids = ids.lock().expect("persistent job id lock poisoned");
                if let Some(job) = jobs.iter().find(|job| ids.contains(&job.id)) {
                    return Err(SubmitError::DuplicateJob(job.id));
                }
            }
            let mut tokens = self.tokens.lock().expect("job token lock poisoned");
            if let Some(job) = jobs.iter().find(|job| tokens.contains_key(&job.id)) {
                return Err(SubmitError::DuplicateJob(job.id));
            }
            reserve(&self.reservations, self.capacity, jobs.len())?;
            for job in jobs {
                tokens.insert(job.id, CancellationToken::new());
            }
            if let Some(ids) = &self.persistent_ids {
                ids.lock()
                    .expect("persistent job id lock poisoned")
                    .extend(job_ids.iter().copied());
            }
        }
        if self.control.send(Control::Submit(graph)).is_err() {
            self.reservations.fetch_sub(job_count, Ordering::AcqRel);
            let mut tokens = self.tokens.lock().expect("job token lock poisoned");
            for job in job_ids {
                tokens.remove(&job);
                if let Some(ids) = &self.persistent_ids {
                    ids.lock()
                        .expect("persistent job id lock poisoned")
                        .remove(&job);
                }
            }
            return Err(SubmitError::Closed);
        }
        Ok(())
    }

    /// Request cooperative cancellation of a live job.
    pub fn cancel(&self, job: JobId) -> bool {
        if let Some(token) = self
            .tokens
            .lock()
            .expect("job token lock poisoned")
            .get(&job)
        {
            token.cancel();
            true
        } else {
            false
        }
    }

    /// Receive the next lifecycle event, or `None` on timeout/disconnect.
    pub fn recv_event_timeout(&self, timeout: Duration) -> Option<JobEvent> {
        self.events
            .lock()
            .expect("job event lock poisoned")
            .recv_timeout(timeout)
            .ok()
    }

    /// Cancel remaining jobs and join scheduler and worker threads.
    pub fn shutdown(mut self) {
        self.stop();
    }

    fn stop(&mut self) {
        let _ = self.control.send(Control::Shutdown);
        if let Some(thread) = self.scheduler_thread.take() {
            let _ = thread.join();
        }
    }
}

fn load_persistent(
    path: &Path,
    capacity: usize,
) -> Result<(PathBuf, Option<JobSnapshot>), SchedulerError> {
    let path = path.to_path_buf();
    let restored = persistence::load_latest(&path).map_err(SchedulerError::Persistence)?;
    let live = restored
        .as_ref()
        .map(|snapshot| {
            snapshot
                .graph()
                .jobs()
                .iter()
                .filter(|job| snapshot.state(job.id) == Some(PersistedJobState::Pending))
                .count()
        })
        .unwrap_or(0);
    if live > capacity {
        return Err(SchedulerError::RestoreExceedsCapacity);
    }
    Ok((path, restored))
}

impl Drop for JobScheduler {
    fn drop(&mut self) {
        self.stop();
    }
}

fn reserve(counter: &AtomicUsize, capacity: usize, amount: usize) -> Result<(), SubmitError> {
    let mut current = counter.load(Ordering::Acquire);
    loop {
        if amount > capacity.saturating_sub(current) {
            return Err(SubmitError::QueueFull);
        }
        match counter.compare_exchange_weak(
            current,
            current + amount,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => return Ok(()),
            Err(actual) => current = actual,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn scheduler_loop(
    worker_count: usize,
    executor: Arc<Executor>,
    controls: Receiver<Control>,
    events: Sender<JobEvent>,
    tokens: Arc<Mutex<HashMap<JobId, CancellationToken>>>,
    reservations: Arc<AtomicUsize>,
    initialized: Sender<Result<(), String>>,
    persistence_path: Option<PathBuf>,
    restored: Option<JobSnapshot>,
) {
    let (work_tx, work_rx) = mpsc::channel::<Option<Work>>();
    let work_rx = Arc::new(Mutex::new(work_rx));
    let (done_tx, done_rx) = mpsc::channel::<WorkDone>();
    let mut workers = Vec::with_capacity(worker_count);
    for index in 0..worker_count {
        let receiver = work_rx.clone();
        let done = done_tx.clone();
        let event_sink = events.clone();
        let run = executor.clone();
        let thread = std::thread::Builder::new()
            .name(format!("pdf-job-worker-{index}"))
            .spawn(move || worker_loop(receiver, done, event_sink, run));
        match thread {
            Ok(thread) => workers.push(thread),
            Err(error) => {
                for _ in &workers {
                    let _ = work_tx.send(None);
                }
                for worker in workers {
                    let _ = worker.join();
                }
                let _ = initialized.send(Err(error.to_string()));
                return;
            }
        }
    }
    let _ = initialized.send(Ok(()));

    let mut pending: HashMap<JobId, Pending> = HashMap::new();
    let mut dependants: HashMap<JobId, Vec<JobId>> = HashMap::new();
    let mut ready = BinaryHeap::new();
    let mut sequence = 0usize;
    let mut running = 0usize;
    let mut shutting_down = false;
    let mut durable = persistence_path.map(|path| DurableQueue::new(path, restored));

    if let Some(snapshot) = durable.as_ref().and_then(|queue| queue.restored.as_ref()) {
        let mut blocked = Vec::new();
        for spec in snapshot.graph().jobs() {
            if snapshot.state(spec.id) != Some(PersistedJobState::Pending) {
                continue;
            }
            let failed_dependency = spec.dependencies.iter().any(|id| {
                matches!(
                    snapshot.state(*id),
                    Some(PersistedJobState::Failed | PersistedJobState::Cancelled)
                )
            });
            let remaining = spec
                .dependencies
                .iter()
                .filter(|id| snapshot.state(**id) == Some(PersistedJobState::Pending))
                .count();
            for dependency in &spec.dependencies {
                if snapshot.state(*dependency) == Some(PersistedJobState::Pending) {
                    dependants.entry(*dependency).or_default().push(spec.id);
                }
            }
            if failed_dependency {
                blocked.push(spec.id);
            } else if remaining == 0 {
                ready.push(Ready {
                    priority: spec.priority,
                    sequence,
                    job: spec.id,
                });
                sequence += 1;
            }
            pending.insert(
                spec.id,
                Pending {
                    spec: spec.clone(),
                    remaining,
                    blocked: failed_dependency,
                    crash_retries: 0,
                },
            );
            let _ = events.send(JobEvent::Queued { job: spec.id });
        }
        for id in blocked {
            if let Some(queue) = durable.as_mut() {
                queue.states.insert(id, PersistedJobState::Failed);
                queue.checkpoint(&events);
            }
            let _ = events.send(JobEvent::Failed {
                job: id,
                message: "dependency did not complete".into(),
            });
            finish_job(
                id,
                false,
                &mut pending,
                &dependants,
                &mut ready,
                &mut sequence,
                &events,
                &tokens,
                &reservations,
                &mut durable,
            );
        }
    }
    if let Some(queue) = durable.as_mut() {
        queue.restored = None;
    }

    loop {
        while let Ok(done) = done_rx.try_recv() {
            running = running.saturating_sub(1);
            if !done.cancelled {
                if let Err(JobRunError::WorkerCrashed(message)) = &done.result {
                    let retry = pending
                        .get_mut(&done.job)
                        .filter(|pending| pending.spec.idempotent && pending.crash_retries == 0);
                    if let Some(pending_job) = retry {
                        pending_job.crash_retries = 1;
                        let _ = events.send(JobEvent::WorkerCrashed {
                            job: done.job,
                            will_retry: true,
                            message: message.clone(),
                        });
                        ready.push(Ready {
                            priority: pending_job.spec.priority,
                            sequence,
                            job: done.job,
                        });
                        sequence += 1;
                        if let Some(queue) = durable.as_mut() {
                            queue.states.insert(done.job, PersistedJobState::Pending);
                            queue.checkpoint(&events);
                        }
                        continue;
                    }
                    let _ = events.send(JobEvent::WorkerCrashed {
                        job: done.job,
                        will_retry: false,
                        message: message.clone(),
                    });
                }
            }
            let success = !done.cancelled && done.result.is_ok();
            let event = if done.cancelled {
                JobEvent::Cancelled { job: done.job }
            } else if let Err(error) = done.result {
                JobEvent::Failed {
                    job: done.job,
                    message: match error {
                        JobRunError::Execution(message) | JobRunError::WorkerCrashed(message) => {
                            message
                        }
                    },
                }
            } else {
                JobEvent::Completed { job: done.job }
            };
            let _ = events.send(event);
            if let Some(queue) = durable.as_mut() {
                queue.states.insert(
                    done.job,
                    if done.cancelled {
                        PersistedJobState::Cancelled
                    } else if success {
                        PersistedJobState::Completed
                    } else {
                        PersistedJobState::Failed
                    },
                );
                queue.checkpoint(&events);
            }
            finish_job(
                done.job,
                success,
                &mut pending,
                &dependants,
                &mut ready,
                &mut sequence,
                &events,
                &tokens,
                &reservations,
                &mut durable,
            );
        }

        match controls.recv_timeout(Duration::from_millis(2)) {
            Ok(Control::Submit(graph)) if !shutting_down => {
                if let Some(queue) = durable.as_mut() {
                    for spec in graph.jobs() {
                        queue.jobs.push(spec.clone());
                        queue.states.insert(spec.id, PersistedJobState::Pending);
                    }
                    queue.checkpoint(&events);
                }
                for spec in graph.into_jobs() {
                    let _ = events.send(JobEvent::Queued { job: spec.id });
                    for dependency in &spec.dependencies {
                        dependants.entry(*dependency).or_default().push(spec.id);
                    }
                    let remaining = spec.dependencies.len();
                    if remaining == 0 {
                        ready.push(Ready {
                            priority: spec.priority,
                            sequence,
                            job: spec.id,
                        });
                        sequence += 1;
                    }
                    pending.insert(
                        spec.id,
                        Pending {
                            spec,
                            remaining,
                            blocked: false,
                            crash_retries: 0,
                        },
                    );
                }
            }
            Ok(Control::Shutdown) | Err(RecvTimeoutError::Disconnected) => {
                shutting_down = true;
                for token in tokens.lock().expect("job token lock poisoned").values() {
                    token.cancel();
                }
            }
            Err(RecvTimeoutError::Timeout) | Ok(Control::Submit(_)) => {}
        }

        while running < workers.len() {
            let Some(item) = ready.pop() else { break };
            let Some(pending_job) = pending.get(&item.job) else {
                continue;
            };
            let token = tokens
                .lock()
                .expect("job token lock poisoned")
                .get(&item.job)
                .cloned();
            let Some(token) = token else { continue };
            if token.is_cancelled() {
                let _ = events.send(JobEvent::Cancelled { job: item.job });
                finish_job(
                    item.job,
                    false,
                    &mut pending,
                    &dependants,
                    &mut ready,
                    &mut sequence,
                    &events,
                    &tokens,
                    &reservations,
                    &mut durable,
                );
                continue;
            }
            if work_tx
                .send(Some(Work {
                    spec: pending_job.spec.clone(),
                    token,
                }))
                .is_ok()
            {
                running += 1;
                if let Some(queue) = durable.as_mut() {
                    queue.states.insert(item.job, PersistedJobState::Running);
                    queue.checkpoint(&events);
                }
            }
        }

        if shutting_down && pending.is_empty() && running == 0 {
            break;
        }
    }
    for _ in &workers {
        let _ = work_tx.send(None);
    }
    for worker in workers {
        let _ = worker.join();
    }
}

fn worker_loop(
    work: Arc<Mutex<Receiver<Option<Work>>>>,
    done: Sender<WorkDone>,
    events: Sender<JobEvent>,
    executor: Arc<Executor>,
) {
    loop {
        let next = work.lock().expect("job work lock poisoned").recv();
        let Ok(Some(work)) = next else { break };
        let _ = events.send(JobEvent::Started { job: work.spec.id });
        let context = JobContext {
            job: work.spec.id,
            token: work.token.clone(),
            events: events.clone(),
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            executor(work.spec.clone(), context)
        }))
        .unwrap_or_else(|_| Err(JobRunError::Execution("job executor panicked".into())));
        let _ = done.send(WorkDone {
            job: work.spec.id,
            cancelled: work.token.is_cancelled(),
            result,
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn finish_job(
    job: JobId,
    success: bool,
    pending: &mut HashMap<JobId, Pending>,
    dependants: &HashMap<JobId, Vec<JobId>>,
    ready: &mut BinaryHeap<Ready>,
    sequence: &mut usize,
    events: &Sender<JobEvent>,
    tokens: &Arc<Mutex<HashMap<JobId, CancellationToken>>>,
    reservations: &Arc<AtomicUsize>,
    durable: &mut Option<DurableQueue>,
) {
    if pending.remove(&job).is_none() {
        return;
    }
    tokens.lock().expect("job token lock poisoned").remove(&job);
    reservations.fetch_sub(1, Ordering::AcqRel);
    let mut blocked = Vec::new();
    for dependant in dependants.get(&job).into_iter().flatten() {
        if let Some(next) = pending.get_mut(dependant) {
            next.remaining = next.remaining.saturating_sub(1);
            next.blocked |= !success;
            if next.remaining == 0 {
                if next.blocked {
                    blocked.push(*dependant);
                } else {
                    ready.push(Ready {
                        priority: next.spec.priority,
                        sequence: *sequence,
                        job: *dependant,
                    });
                    *sequence += 1;
                }
            }
        }
    }
    for dependant in blocked {
        if let Some(queue) = durable.as_mut() {
            queue.states.insert(dependant, PersistedJobState::Failed);
            queue.checkpoint(events);
        }
        let _ = events.send(JobEvent::Failed {
            job: dependant,
            message: "dependency did not complete".into(),
        });
        finish_job(
            dependant,
            false,
            pending,
            dependants,
            ready,
            sequence,
            events,
            tokens,
            reservations,
            durable,
        );
    }
}

struct DurableQueue {
    path: PathBuf,
    jobs: Vec<JobSpec>,
    states: HashMap<JobId, PersistedJobState>,
    restored: Option<JobSnapshot>,
}

impl DurableQueue {
    fn new(path: PathBuf, restored: Option<JobSnapshot>) -> Self {
        let (jobs, states) = restored
            .clone()
            .map(|snapshot| {
                let (graph, states) = snapshot.into_parts();
                (graph.into_jobs(), states)
            })
            .unwrap_or_default();
        Self {
            path,
            jobs,
            states,
            restored,
        }
    }

    fn checkpoint(&self, events: &Sender<JobEvent>) {
        let result = JobGraph::new(self.jobs.clone())
            .map_err(PersistenceError::Graph)
            .and_then(|graph| JobSnapshot::new(graph, self.states.clone()))
            .and_then(|snapshot| persistence::append_snapshot(&self.path, &snapshot));
        if let Err(error) = result {
            let _ = events.send(JobEvent::PersistenceFailed {
                message: format!("{error:?}"),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::time::Duration;

    fn job(id: u64) -> JobSpec {
        JobSpec::new(id, format!("job-{id}"), JobPriority::UserInitiated)
    }

    #[test]
    fn graph_rejects_duplicate_ids() {
        assert_eq!(
            JobGraph::new(vec![job(1), job(1)]).unwrap_err(),
            GraphError::DuplicateJob(1)
        );
    }

    #[test]
    fn graph_rejects_missing_dependency() {
        assert_eq!(
            JobGraph::new(vec![job(1).depends_on(99)]).unwrap_err(),
            GraphError::MissingDependency {
                job: 1,
                dependency: 99
            }
        );
    }

    #[test]
    fn graph_rejects_cycles() {
        assert_eq!(
            JobGraph::new(vec![job(1).depends_on(2), job(2).depends_on(1)]).unwrap_err(),
            GraphError::Cycle
        );
    }

    #[test]
    fn cancellation_token_is_shared() {
        let token = CancellationToken::new();
        let copy = token.clone();
        assert!(!copy.is_cancelled());
        token.cancel();
        assert!(copy.is_cancelled());
    }

    #[test]
    fn scheduler_runs_dependencies_before_dependants() {
        let order = Arc::new(Mutex::new(Vec::new()));
        let seen = order.clone();
        let scheduler = JobScheduler::new(1, 8, move |job, _| {
            seen.lock().unwrap().push(job.id);
            Ok(())
        })
        .unwrap();
        scheduler
            .submit(JobGraph::new(vec![job(2).depends_on(1), job(1)]).unwrap())
            .unwrap();
        assert_eq!(terminal_events(&scheduler, 2), 2);
        assert_eq!(*order.lock().unwrap(), vec![1, 2]);
        scheduler.shutdown();
    }

    #[test]
    fn scheduler_prefers_higher_priority_ready_jobs() {
        let order = Arc::new(Mutex::new(Vec::new()));
        let seen = order.clone();
        let scheduler = JobScheduler::new(1, 8, move |job, _| {
            seen.lock().unwrap().push(job.id);
            Ok(())
        })
        .unwrap();
        scheduler
            .submit(
                JobGraph::new(vec![
                    JobSpec::new(1, "maintenance", JobPriority::Maintenance),
                    JobSpec::new(2, "interactive", JobPriority::InteractiveAdjacent),
                ])
                .unwrap(),
            )
            .unwrap();
        assert_eq!(terminal_events(&scheduler, 2), 2);
        assert_eq!(*order.lock().unwrap(), vec![2, 1]);
        scheduler.shutdown();
    }

    #[test]
    fn scheduler_reports_queue_full() {
        let scheduler = JobScheduler::new(1, 1, |_job, _| {
            std::thread::sleep(Duration::from_millis(100));
            Ok(())
        })
        .unwrap();
        scheduler
            .submit(JobGraph::new(vec![job(1)]).unwrap())
            .unwrap();
        assert_eq!(
            scheduler.submit(JobGraph::new(vec![job(2)]).unwrap()),
            Err(SubmitError::QueueFull)
        );
        assert_eq!(terminal_events(&scheduler, 1), 1);
        scheduler.shutdown();
    }

    #[test]
    fn scheduler_cancellation_is_terminal_and_observable() {
        let scheduler = JobScheduler::new(1, 2, |_job, context| {
            while !context.is_cancelled() {
                std::thread::yield_now();
            }
            Ok(())
        })
        .unwrap();
        scheduler
            .submit(JobGraph::new(vec![job(1)]).unwrap())
            .unwrap();
        loop {
            if matches!(
                scheduler.recv_event_timeout(Duration::from_secs(1)),
                Some(JobEvent::Started { job: 1 })
            ) {
                break;
            }
        }
        assert!(scheduler.cancel(1));
        loop {
            if matches!(
                scheduler.recv_event_timeout(Duration::from_secs(1)),
                Some(JobEvent::Cancelled { job: 1 })
            ) {
                break;
            }
        }
        scheduler.shutdown();
    }

    #[test]
    fn scheduler_turns_executor_panic_into_failure() {
        let scheduler =
            JobScheduler::new(1, 1, |_job, _| -> Result<(), String> { panic!("boom") }).unwrap();
        scheduler
            .submit(JobGraph::new(vec![job(1)]).unwrap())
            .unwrap();
        loop {
            match scheduler.recv_event_timeout(Duration::from_secs(2)) {
                Some(JobEvent::Failed { job: 1, message }) => {
                    assert!(message.contains("panicked"));
                    break;
                }
                Some(_) => {}
                None => panic!("missing terminal failure event"),
            }
        }
        scheduler.shutdown();
    }

    #[test]
    fn persistent_scheduler_restores_only_unfinished_jobs() {
        use persistence::{append_snapshot, load_latest, JobSnapshot, PersistedJobState};
        use std::collections::HashMap;

        let path = temp_path();
        let graph = JobGraph::new(vec![job(1), job(2).depends_on(1)]).unwrap();
        append_snapshot(
            &path,
            &JobSnapshot::new(
                graph,
                HashMap::from([
                    (1, PersistedJobState::Completed),
                    (2, PersistedJobState::Pending),
                ]),
            )
            .unwrap(),
        )
        .unwrap();
        let ran = Arc::new(Mutex::new(Vec::new()));
        let seen = ran.clone();
        let scheduler = JobScheduler::new_persistent(1, 4, &path, move |spec, _| {
            seen.lock().unwrap().push(spec.id);
            Ok(())
        })
        .unwrap();
        assert_eq!(terminal_events(&scheduler, 1), 1);
        scheduler.shutdown();
        assert_eq!(*ran.lock().unwrap(), vec![2]);
        assert_eq!(
            load_latest(&path).unwrap().unwrap().state(2),
            Some(PersistedJobState::Completed)
        );
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn persistent_scheduler_rejects_restore_over_capacity() {
        use persistence::{append_snapshot, JobSnapshot, PersistedJobState};
        use std::collections::HashMap;

        let path = temp_path();
        let graph = JobGraph::new(vec![job(1), job(2)]).unwrap();
        append_snapshot(
            &path,
            &JobSnapshot::new(
                graph,
                HashMap::from([
                    (1, PersistedJobState::Pending),
                    (2, PersistedJobState::Pending),
                ]),
            )
            .unwrap(),
        )
        .unwrap();
        let result = JobScheduler::new_persistent(1, 1, &path, |_spec, _| Ok(()));
        assert!(matches!(
            result,
            Err(SchedulerError::RestoreExceedsCapacity)
        ));
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn persistent_scheduler_fails_dependant_of_failed_restored_job() {
        use persistence::{append_snapshot, JobSnapshot, PersistedJobState};
        use std::collections::HashMap;

        let path = temp_path();
        append_snapshot(
            &path,
            &JobSnapshot::new(
                JobGraph::new(vec![job(1), job(2).depends_on(1)]).unwrap(),
                HashMap::from([
                    (1, PersistedJobState::Failed),
                    (2, PersistedJobState::Pending),
                ]),
            )
            .unwrap(),
        )
        .unwrap();
        let scheduler = JobScheduler::new_persistent(1, 2, &path, |_spec, _| {
            panic!("blocked job must not execute")
        })
        .unwrap();
        assert_eq!(terminal_events(&scheduler, 1), 1);
        scheduler.shutdown();
        assert_eq!(
            persistence::load_latest(&path).unwrap().unwrap().state(2),
            Some(PersistedJobState::Failed)
        );
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn idempotent_job_retries_one_worker_crash() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let seen = attempts.clone();
        let scheduler = JobScheduler::new_typed(1, 1, move |_spec, _| {
            if seen.fetch_add(1, Ordering::Relaxed) == 0 {
                Err(JobRunError::WorkerCrashed("utility exited".into()))
            } else {
                Ok(())
            }
        })
        .unwrap();
        scheduler
            .submit(JobGraph::new(vec![job(1).idempotent()]).unwrap())
            .unwrap();
        assert_eq!(terminal_events(&scheduler, 1), 1);
        assert_eq!(attempts.load(Ordering::Relaxed), 2);
        scheduler.shutdown();
    }

    #[test]
    fn non_idempotent_job_does_not_retry_worker_crash() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let seen = attempts.clone();
        let scheduler = JobScheduler::new_typed(1, 1, move |_spec, _| {
            seen.fetch_add(1, Ordering::Relaxed);
            Err(JobRunError::WorkerCrashed("utility exited".into()))
        })
        .unwrap();
        scheduler
            .submit(JobGraph::new(vec![job(1)]).unwrap())
            .unwrap();
        assert_eq!(terminal_events(&scheduler, 1), 1);
        assert_eq!(attempts.load(Ordering::Relaxed), 1);
        scheduler.shutdown();
    }

    #[test]
    fn idempotent_job_retries_worker_crash_only_once() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let seen = attempts.clone();
        let scheduler = JobScheduler::new_typed(1, 1, move |_spec, _| {
            seen.fetch_add(1, Ordering::Relaxed);
            Err(JobRunError::WorkerCrashed("utility exited".into()))
        })
        .unwrap();
        scheduler
            .submit(JobGraph::new(vec![job(1).idempotent()]).unwrap())
            .unwrap();
        assert_eq!(terminal_events(&scheduler, 1), 1);
        assert_eq!(attempts.load(Ordering::Relaxed), 2);
        scheduler.shutdown();
    }

    fn temp_path() -> std::path::PathBuf {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        std::env::temp_dir().join(format!(
            "pdf-platform-scheduler-{}-{}.log",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn terminal_events(scheduler: &JobScheduler, expected: usize) -> usize {
        let mut terminals = 0;
        while terminals < expected {
            match scheduler.recv_event_timeout(Duration::from_secs(2)) {
                Some(
                    JobEvent::Completed { .. }
                    | JobEvent::Failed { .. }
                    | JobEvent::Cancelled { .. },
                ) => terminals += 1,
                Some(_) => {}
                None => break,
            }
        }
        terminals
    }
}
