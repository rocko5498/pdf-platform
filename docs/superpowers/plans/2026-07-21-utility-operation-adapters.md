# Utility Operation Adapters Implementation Plan

**Goal:** Complete the shared utility-worker execution path for OCR, thumbnails, indexing, and
optimization without granting workers ambient filesystem/network access or bypassing the
coordinator's single-writer rule.

**Requirements:** ADR-005, ADR-006, ADR-008, ADR-009, ADR-011, ADR-018, ADR-019, ADR-021,
ADR-031; SDS §2.2.3, §2.4, §2.5, §4.2, §4.6, §6, §8, §12; FR-BATCH-3,
FR-BATCH-4, FR-OCR-1, NFR-MEM-3, NFR-RESP.

## Scope and ordering

The four documented utility operations share two prerequisites: opaque capability grants and a
bounded bulk-data path. Implement those once, then add operation adapters in risk order. OCR is
first because its engine trait and normalized result already exist. Thumbnailing reuses raster
infrastructure. Indexing consumes the canonical text model. Optimization is last because it writes
a new file and therefore needs the strongest broker and verification gates.

## Task 1 — Capability and bulk-input contract

**Status:** Complete on `codex/jobs-scheduler`; security review required before merge.

**Files:** `core/protocol/src/utility_jobs.rs`, `core/jobs/src/utility_pool.rs`,
`core/coordinator/src/broker.rs`, focused tests.

- Add opaque, process-local grant IDs; never serialize user paths into Z1 messages.
- Add typed input descriptors for bounded inline control data and shared-memory regions.
- Validate offset + length with checked arithmetic before every Z1 read/write.
- Add typed broker request/denial events; unknown, expired, or wrong-capability grants fail closed.
- Declare per-worker shared-memory capacity and reject jobs exceeding it (`GR-7`, `NFR-MEM-3`).
- Tests: codec bounds, forged grants, expired grants, out-of-bounds descriptors, cancellation,
  worker replacement invalidating process-local grants.

**Gate:** No operation adapter starts until forged and out-of-bounds inputs are rejected in tests.

**Evidence:** forged/expired/wrong-scope grants fail closed; real worker replacement revokes its
grants; fixed per-slot shared memory is inherited without paths; real IPC rejects an out-of-bounds
descriptor. Workspace total: 531 passing tests.

## Task 2 — OCR recognition adapter

**Files:** `core/ocr-bridge`, `core/worker-main`, `core/coordinator/src/document.rs`, protocol tests.

- Document worker renders the requested page; Z0 copies the validated raster into the utility
  worker's bounded shared-memory slot.
- Utility worker invokes only `OcrEngine`; it returns the normalized text/boxes/confidence model.
- Coordinator validates page identity, geometry, text lengths, and confidence ranges.
- The utility result never mutates the document directly. Coordinator converts it into the
  existing invisible-text-layer Command (`GR-2`, ADR-006).
- Emit progress/cancellation at render, recognition, validation, and command-production stages.
- Tests: real IPC OCR fixture, malformed geometry rejection, low-confidence honesty, cancellation,
  worker crash retry only when recognition is idempotent, JBIG2 symbol mode remains off.

**Gate:** Searchable output and text-layer registration pass the scan corpus before claiming M9.

## Task 3 — Thumbnail adapter

**Files:** `core/render-pipeline`, `core/worker-main`, `core/jobs`, coordinator integration.

- Use the engine capability seam; never call PDFium directly from the scheduler.
- Return pixels only through bounded shared memory with generation/revision keys.
- Apply maintenance priority and cache eviction/accounting under `MemoryGovernor`.
- Tests: stale generation discard, descriptor bounds, cancellation, large-document cache bound,
  document-worker interactive render remains higher priority.

**Gate:** Thumbnail farms cannot exceed their declared cache/shared-memory budgets.

## Task 4 — Cross-document indexing adapter

**Files:** `core/search`, `core/jobs`, coordinator broker/grant integration.

- Accept only explicitly enrolled folder/file grants; no ambient directory walking in Z1.
- Consume the canonical extraction model from ADR-019; do not add a second extractor.
- Keep the index local, budgeted, inspectable, and deletable; record source revision identity.
- Tests: non-enrolled paths denied, changed-file invalidation, unreliable extraction flagged,
  cancellation/resume, index size ceiling.

**Gate:** No file is indexed without an explicit active enrollment grant.

## Task 5 — Optimization adapter and brokered output

**Files:** `core/pdf-write`, `core/coordinator/src/broker.rs`, jobs/protocol integration.

- Broker creates the destination and passes only scoped handles/capabilities.
- Utility worker writes a candidate output; it never replaces the source itself.
- Coordinator verifies candidate structure/conformance and performs the final atomic publish.
- Preserve incremental-save default; optimization remains explicit and non-destructive (`GR-5`).
- Tests: source unchanged on crash/cancel/disk-full, forged output grant denied, candidate verification
  failure blocks publish, byte-for-byte recovery behavior.

**Gate:** Fault-injection suite proves the original remains intact across every failure point.

## Task 6 — Product wiring and milestone closure

- Route GUI and CLI through the same declarative job constructors (`ADR-025`, FR-BATCH).
- Persist resumable inputs as stable grant recipes, not live process handles; re-broker after restart.
- Add per-file results and summary reports; continue past isolated batch failures.
- Run unit/property, real IPC, corpus, differential, fault-injection, and platform confinement tests.
- Obtain mandatory human review for sandbox/broker changes before merge.
- Update `docs/milestone-exit-tracker.md` only with evidence actually produced.

## Planned commits

1. `feat: add scoped utility input grants`
2. `feat: run OCR recognition in utility workers`
3. `feat: schedule bounded thumbnail jobs`
4. `feat: index explicitly enrolled documents`
5. `feat: broker verified optimization outputs`
6. `test: close utility operation milestone gates`

Each commit body cites the applicable FR/NFR/ADR/SDS IDs. Security-path commits are drafts until
the required human confinement review is recorded.
