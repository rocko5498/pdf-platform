# Utility Worker Pool Plan

- [x] Add bounded utility job command/result codecs and round-trip/rejection tests.
- [x] Dispatch utility envelopes in worker-main with honest unsupported-operation failures.
- [x] Add a fixed-size sandbox-spawned pool that maps process/protocol loss to `WorkerCrashed`.
- [x] Verify protocol, jobs, worker, clippy, formatting, and workspace tests.
