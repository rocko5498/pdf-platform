# Utility Worker OS Priority Plan

- [x] Add an explicit low-priority utility spawn API while preserving normal document spawn.
- [x] Apply below-normal priority on Windows and nice level 10 on POSIX.
- [x] Route every utility-pool spawn and replacement through the new policy.
- [x] Run sandbox/jobs/worker tests, real-process IPC smoke, clippy, and workspace tests.
