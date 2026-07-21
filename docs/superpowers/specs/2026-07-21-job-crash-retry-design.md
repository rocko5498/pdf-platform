# Utility Job Crash Retry Design

**Requirements:** ADR-008, ADR-009, SDS §2.2.3, SDS §4.6

The scheduler needs a typed distinction between an operation failure and loss of the utility
worker that was executing it. Existing string-returning executors remain supported and map to
ordinary operation failures. A typed executor may return `WorkerCrashed`; the scheduler emits a
structured crash event and retries exactly once only when the submitted `JobSpec` explicitly
declares itself idempotent. Non-idempotent jobs and a second crash fail normally.

Idempotency is part of the declarative job description and therefore part of the persisted wire
format. The codec advances to version 2 while retaining version-1 read compatibility, where old
jobs restore as non-idempotent. Retry attempts are process-local: if the application itself exits,
the interrupted job restores pending and receives a fresh single retry allowance.

This slice establishes the scheduler/utility-worker contract. It does not claim a utility process
pool, brokered file outputs, or operation-specific worker dispatch.
