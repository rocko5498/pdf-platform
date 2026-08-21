# Stamp Injection Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make CLI and batch watermark/Bates operations write real incremental PDF stamp revisions.

**Architecture:** Add minimal page-dictionary patch helpers in `pdf-model`; compose those objects into one coordinator-owned command group; route CLI and batch through the coordinator. Unsupported PDF shapes fail honestly.

**Tech Stack:** Rust standard library, existing `pdf-model`, `coordinator`, `pdf-write`, and worker session APIs.

## Global Constraints

- Preserve ADR-012 incremental saving and ADR-013 command-only mutation.
- Add no dependency and no `unsafe`.
- Do not report success unless an output containing the new stamp objects is written.

---

### Task 1: Page stamp patch primitives

**Files:**
- Modify: `core/pdf-model/src/page_patch.rs`
- Test: `core/pdf-model/src/page_patch.rs`

- [x] Add failing tests for indirect and array `/Contents`, direct resource font injection, and unsupported indirect resources.
- [x] Run `cargo test -p pdf-model page_patch` and confirm the new tests fail because the helper is absent.
- [x] Implement the smallest `inject_content_ref_and_font` helper.
- [x] Re-run the focused tests and confirm they pass.

### Task 2: Coordinator command path

**Files:**
- Modify: `core/coordinator/src/document.rs`
- Test: `core/coordinator/src/document.rs`

- [x] Add a failing command-group test that proves page, font, and content objects are written.
- [x] Run the focused coordinator test and confirm failure because the stamp builder is absent.
- [x] Implement `apply_stamp` using existing page-tree lookup, stamp stream generation, `SetObjectCommand`, and `apply_command_group`.
- [x] Re-run the focused test and confirm it passes.

### Task 3: CLI and batch routing

**Files:**
- Modify: `core/cli/src/main.rs`
- Modify: `core/pdf-model/src/batch.rs`
- Modify: `core/pdf-model/Cargo.toml` only if required by the existing crate direction; otherwise keep batch routing in CLI.

- [x] Add a failing CLI test proving stamp output differs from input and contains the stamp.
- [x] Replace the CLI report-only branch with coordinator open, stamp, and incremental save.
- [x] Route batch watermark/Bates through the same callable path without introducing a dependency cycle; orchestration remains in CLI and `pdf-model` remains declarative.
- [x] Run focused CLI, batch, coordinator, and pdf-model tests.

### Task 4: Verification and documentation

**Files:**
- Modify: `docs/milestone-exit-tracker.md`
- Modify: `README.md` only if user-visible behavior changes its status text.

- [ ] Run `cargo fmt --check`.
- [x] Run `cargo test -p pdf-model` and `cargo test -p coordinator`.
- [x] Run the CLI stamp command on a fixture and inspect the output bytes.
- [x] Update the tracker with only the verified evidence.
