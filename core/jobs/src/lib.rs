//! Bounded, cancellable declarative job DAG scheduler. [ADR-009]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::cmp::Ordering as CmpOrdering;
use std::collections::{BinaryHeap, HashMap, VecDeque};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc::{self, Receiver, RecvTimeoutError, Sender},
    Arc, Mutex,
};
use std::thread::JoinHandle;
use std::time::Duration;

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
}

impl JobSpec {
    /// Create a job with no dependencies.
    pub fn new(id: JobId, operation: impl Into<String>, priority: JobPriority) -> Self {
        Self {
            id,
            operation: operation.into(),
            priority,
            dependencies: Vec::new(),
        }
    }

    /// Add a dependency.
    pub fn depends_on(mut self, dependency: JobId) -> Self {
        self.dependencies.push(dependency);
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

type Executor = dyn Fn(JobSpec, JobContext) -> Result<(), String> + Send + Sync + 'static;

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
    result: Result<(), String>,
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
}

/// Bounded scheduler backed by named standard-library worker threads.
pub struct JobScheduler {
    control: Sender<Control>,
    events: Mutex<Receiver<JobEvent>>,
    tokens: Arc<Mutex<HashMap<JobId, CancellationToken>>>,
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
        if worker_count == 0 || capacity == 0 {
            return Err(SchedulerError::ZeroCapacity);
        }
        let executor: Arc<Executor> = Arc::new(executor);
        let (control_tx, control_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let (init_tx, init_rx) = mpsc::channel();
        let tokens = Arc::new(Mutex::new(HashMap::new()));
        let reservations = Arc::new(AtomicUsize::new(0));
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
            let mut tokens = self.tokens.lock().expect("job token lock poisoned");
            if let Some(job) = jobs.iter().find(|job| tokens.contains_key(&job.id)) {
                return Err(SubmitError::DuplicateJob(job.id));
            }
            reserve(&self.reservations, self.capacity, jobs.len())?;
            for job in jobs {
                tokens.insert(job.id, CancellationToken::new());
            }
        }
        if self.control.send(Control::Submit(graph)).is_err() {
            self.reservations.fetch_sub(job_count, Ordering::AcqRel);
            let mut tokens = self.tokens.lock().expect("job token lock poisoned");
            for job in job_ids {
                tokens.remove(&job);
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

fn scheduler_loop(
    worker_count: usize,
    executor: Arc<Executor>,
    controls: Receiver<Control>,
    events: Sender<JobEvent>,
    tokens: Arc<Mutex<HashMap<JobId, CancellationToken>>>,
    reservations: Arc<AtomicUsize>,
    initialized: Sender<Result<(), String>>,
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

    loop {
        while let Ok(done) = done_rx.try_recv() {
            running = running.saturating_sub(1);
            let success = !done.cancelled && done.result.is_ok();
            let event = if done.cancelled {
                JobEvent::Cancelled { job: done.job }
            } else if let Err(message) = done.result {
                JobEvent::Failed {
                    job: done.job,
                    message,
                }
            } else {
                JobEvent::Completed { job: done.job }
            };
            let _ = events.send(event);
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
            );
        }

        match controls.recv_timeout(Duration::from_millis(2)) {
            Ok(Control::Submit(graph)) if !shutting_down => {
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
        .unwrap_or_else(|_| Err("job executor panicked".into()));
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
        );
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
