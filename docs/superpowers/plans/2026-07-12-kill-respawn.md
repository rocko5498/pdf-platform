# Implementation Plan: Kill-respawn with document re-open (M0 slice 6)

> Design: `docs/superpowers/specs/2026-07-12-kill-respawn-design.md`  
**Branch:** `feat/m0-kill-respawn`

### Task 0: branch + design commit
### Task 1: session owns BrokeredFile + `respawn()`
### Task 2: update open_inspect + session_death tests; add kill_respawn_inspect
### Task 3: cargo test + PR + CI + merge

Non-goals: tiles, PDFium, auto-respawn actor, confinement.
