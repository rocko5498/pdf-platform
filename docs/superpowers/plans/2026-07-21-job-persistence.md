# Job Queue Persistence Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a bounded versioned persistence format for declarative job graphs and lifecycle state.

**Architecture:** Append self-contained binary snapshots with a trailing length commit marker; load the last complete frame and validate the reconstructed graph.

**Tech Stack:** Rust standard library only.

## Global Constraints

- No new dependency, unsafe code, overwrite rename, or silent recovery from a malformed complete frame.
- Incomplete trailing frames are ignored; unknown versions fail explicitly.
- Running state restores as pending.

---

### Task 1: Snapshot codec

- [x] Add failing tests for round-trip, running-state normalization, versions, and bounds.
- [x] Implement bounded encode/decode with typed errors.
- [x] Run focused tests.

### Task 2: Append-only store

- [x] Add failing tests for newest-snapshot loading and torn-tail recovery.
- [x] Implement append + `sync_data` and last-complete-frame loading.
- [x] Run jobs tests, clippy, format check, workspace tests, and diff check.

### Task 3: Honest status documentation

- [x] Update the tracker to distinguish format completion from scheduler checkpoint integration.
# Scheduler checkpoint integration

- [x] Add failing tests for restored unfinished jobs, completed dependency handling, capacity, and
  automatic terminal checkpoints.
- [x] Add a persistent scheduler constructor and actor-owned durable state mirror.
- [x] Surface checkpoint failures as diagnostics and verify focused plus workspace test strata.
