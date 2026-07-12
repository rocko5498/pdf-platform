# Implementation Plan: Shmem tile seam (M0 slice 7)

> Design: `docs/superpowers/specs/2026-07-12-shmem-tile-design.md`  
**Branch:** `feat/m0-shmem-tile`

### Task 0: design commit + branch
### Task 1: protocol handles + tile_ready codec + events optional
### Task 2: sandbox::shmem SharedRegion (memmap2 0.9)
### Task 3: spawn inherit SHMEM_FD/HANDLE + adopt_shmem_file
### Task 4: worker tile_smoke + session helper / integration test
### Task 5: cargo test + PR + CI + merge

Non-goals: PDFium, GPU, multi-slot pool, confinement.
