# Job Scheduler Core Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the empty jobs crate with a bounded, cancellable, priority-aware DAG scheduler core.

**Architecture:** Standard-library channels and named worker threads own a bounded queue. Declarative job metadata is separated from an injected executor seam so later utility-process IPC does not change scheduling semantics.

**Tech Stack:** Rust standard library only.

## Global Constraints

- No async runtime, dependency, unsafe code, or forced thread termination.
- Queue capacity and worker count are explicit and non-zero.
- Cancellation is cooperative and always emits a terminal event.

---

### Task 1: Declarative graph and cancellation primitives

- [x] Add failing tests for duplicate IDs, missing dependencies, cycles, and cancellation.
- [x] Run focused tests and confirm failure on absent APIs.
- [x] Implement `JobSpec`, `JobGraph`, validation errors, priority, events, and token.
- [x] Re-run focused tests.

### Task 2: Bounded scheduler

- [x] Add failing tests for dependency order, priority order, queue full, cancellation, and shutdown.
- [x] Run focused tests and confirm failure on absent scheduler.
- [x] Implement fixed worker threads, owned queue, executor callback, event receiver, and shutdown.
- [x] Re-run the crate tests and the workspace tests affected by the API.

### Task 3: Documentation and verification

- [x] Remove stub claims from the crate metadata.
- [x] Update the milestone tracker without claiming persistence or utility-worker IPC.
- [x] Run `cargo test -p jobs`, `cargo test --workspace`, `cargo clippy -p jobs -- -D warnings`, and `git diff --check`.
