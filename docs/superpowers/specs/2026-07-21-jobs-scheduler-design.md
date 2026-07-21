# Job Scheduler Core Design

**Citations:** ADR-009, ADR-010, SDS §2.2.3, SDS §4.6, SDS §5.3

## Scope

Replace the empty `jobs` crate with the smallest useful scheduler core: declarative jobs and DAG dependencies, three ADR priority classes, cooperative cancellation, structured progress/terminal events, and a fixed-size thread pool using standard-library threads and channels.

Persistence, utility-worker IPC, brokered output, crash retry, and plugin quotas remain follow-up integrations. The API and documentation must not claim those capabilities yet.

## Model

- `JobSpec` carries a stable ID, operation name, priority, and dependency IDs.
- `JobGraph` validates duplicate IDs, missing dependencies, and cycles before submission.
- `CancellationToken` is an `Arc<AtomicBool>` checked by executors at safe points.
- `JobContext` exposes cancellation and event emission to a supplied executor.
- `JobScheduler` owns a bounded priority queue and fixed named worker threads. Submission is non-blocking until the queue bound is reached, then returns `QueueFull`.
- `JobEvent` reports queued, started, progress, completed, failed, or cancelled terminal state.

Dependent jobs run only after every dependency completes. A failed or cancelled dependency prevents dependants from running and emits a typed failure.

## Safety and limits

No async runtime, new dependency, unsafe code, forced thread termination, document mutation, or unbounded queue. Shutdown joins all worker threads.

## Tests

Tests prove DAG validation, priority ordering, queue bounds, cooperative cancellation with a terminal event, dependency ordering, and clean shutdown.
