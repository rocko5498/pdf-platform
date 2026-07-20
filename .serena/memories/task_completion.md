# Task completion checks

```
cd core
cargo build --workspace
cargo test --workspace   # corpus-diff needs qpdf on PATH
# if CI-relevant: push and wait 3-OS workflow ci.yml
```

Do not claim M0 complete until tile+IPC+shmem+sandbox+kill-respawn per SDS §14.
