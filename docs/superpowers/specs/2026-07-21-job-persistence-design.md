# Job Queue Persistence Design

**Citations:** ADR-009, ADR-021, SDS §2.2.3, SDS §4.6

## Decision

Persist validated job graphs and per-job lifecycle states in a versioned append-only log. Every snapshot is a self-contained length-delimited frame followed by the same length as a commit marker. The writer appends, flushes, and calls `sync_data`; the reader returns the last complete valid frame and ignores an incomplete trailing frame caused by interruption.

This design is cross-platform using only the standard library and avoids relying on overwrite-style rename semantics that differ on Windows. The file has explicit bounds for total bytes, job count, operation length, and dependency count.

## Data

Each snapshot stores every `JobSpec` field plus one state: pending, running, completed, failed, or cancelled. Restoring converts running jobs to pending because no process-local execution survives restart. Unknown versions, invalid enum tags, malformed lengths, duplicate jobs, missing dependencies, and cycles return typed errors.

## Scope boundary

This slice provides the durable format and load/append API. Scheduler-driven checkpoint timing and utility-worker retry policy remain separate integration work and are not claimed here.

## Tests

Round-trip all fields/states, load the latest of multiple snapshots, recover past a torn trailing frame, reject an unsupported version, enforce bounds, and convert running to pending on restore.
