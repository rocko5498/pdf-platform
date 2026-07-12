# System Design Specification (SDS)

**Project:** Open-source professional PDF platform
**Companion document:** *Engineering Constitution* (ADR-001 … ADR-030). This SDS implements those decisions; it does not re-litigate them. References appear as `[ADR-NNN]`.
**Audience:** Senior engineers implementing and maintaining the system over a five-to-ten-year horizon.
**Status:** Baseline design. Sections are normative unless marked *Informative*. New commitments beyond the ADRs are marked **[SDS decision]** and should be ratified alongside this document.
**Notation:** No source code. Data shapes are described structurally (fields, invariants). "Z0/Z1/Z2/Z3" are the trust zones from `[ADR-016]`. "Coordinator", "worker", "shell" carry their ADR meanings.

---

## Table of Contents

1. System Overview
2. Component Architecture
3. Document Lifecycle
4. Data Flow
5. Event System
6. Rendering Pipeline
7. Threading Model
8. Cache Design
9. Memory Lifecycle
10. Error Recovery
11. Plugin Lifecycle
12. Security Model
13. Build System
14. Development Roadmap

---

# 1. System Overview

## 1.1 What the system is, in one paragraph

The application is a **multi-process native desktop PDF platform**. A trusted **UI process** hosts a thin Qt Widgets shell (Z0, C++) bonded to a Rust **coordinator** (Z0) that owns all document truth, scheduling, and state. Untrusted document interpretation and rasterization run in **sandboxed worker processes** (Z1, Rust hosting the PDFium engine and, optionally, a JS interpreter). A shared, low-priority **utility worker pool** (Z1) runs OCR, indexing, optimization, and thumbnails. **Plugins** run as WASM components inside utility workers (Z2). The shell and coordinator communicate over a single typed FFI bridge `[ADR-004]`; the coordinator and workers communicate over an IPC transport with a shared-memory side channel for bulk pixel data. The document itself is always treated as adversarial data (Z3).

The design's spine is one idea from `[ADR-006]`: **a document is a memory-mapped, lazily-materialized object store with a copy-on-write overlay.** Every other subsystem — rendering, editing, undo, save, recovery, history — is a consumer or producer of that overlay.

## 1.2 Process topology (Informative)

```
┌──────────────────────────── UI PROCESS (Z0, trusted) ─────────────────────────────┐
│                                                                                     │
│   Qt Widgets Shell (C++)                    Rust Coordinator                        │
│   ┌──────────────────────┐   cxx bridge   ┌───────────────────────────────────┐    │
│   │ chrome / canvas /     │◀──────────────▶│ DocumentCoordinator (per doc)     │    │
│   │ panels / dialogs /a11y│  cmд / event   │ RenderScheduler                   │    │
│   │ GPU tile compositor   │  + shmem tiles │ JobScheduler                      │    │
│   └──────────────────────┘                │ CacheGovernor / MemoryGovernor    │    │
│                                            │ UndoJournal (per doc)             │    │
│                                            │ PluginHost (control plane)        │    │
│                                            │ Broker (privileged ops)           │    │
│                                            └───────────────┬───────────────────┘    │
└────────────────────────────────────────────────────────────┼──────────────────────┘
                                       IPC (control) + shared memory (bulk)
              ┌───────────────────────────────┼───────────────────────────────┐
              ▼                               ▼                               ▼
┌───────────────────────┐     ┌───────────────────────┐     ┌──────────────────────────┐
│ DOCUMENT WORKER (Z1)   │     │ DOCUMENT WORKER (Z1)   │     │ UTILITY WORKER POOL (Z1)  │
│ one per open document  │ ... │ one per open document  │     │ shared, low priority      │
│  • engine (PDFium)     │     │                        │     │  • OCR / index / optimize │
│  • content interpret   │     │                        │     │  • thumbnail farm         │
│  • text extraction     │     │                        │     │  • WASM plugin host (Z2)  │
│  • PDF JS (subset)     │     │                        │     │    each plugin sandboxed  │
│  sandbox: seccomp/     │     │                        │     │    inside the OS sandbox  │
│  AppContainer/Sandbox  │     │                        │     │                          │
└───────────────────────┘     └───────────────────────┘     └──────────────────────────┘
```

Key properties: a malformed document can crash only its own worker `[ADR-008, ADR-021]`; no Z1/Z2 process has network access or unbrokered filesystem access `[ADR-016]`; the coordinator is the only component that mutates document truth.

## 1.3 Lifecycle from startup to shutdown (Informative narrative)

**Cold start.** The OS launches the UI process. The shell's `app/` initializes Qt, restores window geometry and UI-profile settings (local only), and constructs the coordinator in-process. The coordinator initializes the MemoryGovernor (sizing caches to system RAM), CacheGovernor, JobScheduler, and Broker, then signals *ready* to the shell. **No worker is spawned yet** — cold start does not pay for document machinery. Target: first interactive frame well within the cold-start budget `[ADR-023]`. Startup does zero network I/O `[ADR-016]`.

**Opening a document.** The user opens a file (dialog, file-association, or CLI handoff). The shell issues an `OpenDocument` command; the coordinator brokers the file handle, spawns a document worker, memory-maps the file, and drives the open sequence (§3.1). First page renders progressively (§6). Multiple documents = multiple coordinators + workers, independent in fate and memory accounting.

**Steady state.** The user scrolls, zooms, searches, annotates, fills forms. The shell translates input into commands; the coordinator updates the overlay and schedules renders; workers rasterize into shared memory; the shell composites on the GPU. Long operations become jobs with progress/cancel. Every mutation is a Command appended to the per-document journal, which is periodically persisted for crash recovery.

**Saving.** Default save serializes the CoW overlay as an incremental update appended to the file `[ADR-012]`; O(change) latency; signatures preserved. "Save As Clean" is the explicit full-rewrite path.

**Closing a document.** The coordinator flushes/clears the journal on clean save, tears down the worker, releases that document's cache budget back to the governor, and drops the coordinator. Other documents are unaffected.

**Shutdown.** The shell requests coordinator shutdown; the coordinator cancels jobs, terminates workers gracefully (SIGTERM-equivalent, then force after a deadline), persists nothing that would leak (sidecar journals for cleanly-closed docs are already deleted), and exits. Unclean shutdown is caught at next launch by journal presence (§10).

## 1.4 Design invariants (Normative — these hold everywhere)

1. **Single writer.** Only the owning DocumentCoordinator mutates a document's model/overlay. Everything else reads snapshots or requests mutations via Commands.
2. **No domain logic in Z0's C++.** The shell parses no PDF syntax and stores no document objects `[ADR-003, ADR-026]`.
3. **No trust in Z1/Z2 output.** All data returned from workers/plugins is validated at the coordinator boundary before affecting Z0 state.
4. **Every privilege is brokered.** File, print, clipboard, network — requested by lower zones, executed in Z0 after validation `[ADR-016]`.
5. **Every mutation is a Command; every Command is journaled.** No side-door mutation exists, including from JS `[ADR-017]` and plugins `[ADR-014]`.
6. **Every cache is governed.** No unbounded, unaccounted growth `[ADR-011]`.
7. **The protocol is the only cross-language/cross-process contract.** The shell, the CLI, and (transitively) plugins are peer clients of the same core surface.

---

# 2. Component Architecture

Each component below lists: **responsibility**, **zone/process**, **owns/consumes**, **key interfaces**, and **failure behavior**. Components map to crates/directories per `[ADR-025, ADR-026]`.

## 2.1 Qt Shell (Z0, UI process, C++)

**Responsibility.** Present chrome, canvas, panels, dialogs, accessibility; translate user input into commands; composite tiles; render selection/annotation overlays from geometry. Nothing else.
**Owns.** Window/session/UI-profile state; GPU textures for tiles; transient input state. **Consumes.** The event stream and shared-memory tiles from the coordinator via the bridge.
**Key interfaces.** `bridge/` (sole cxx surface); the versioned shortcut/menu registry (a data file under diff review — the interface-stability contract `[ADR-030]`); the a11y mapping from core structure events to the platform accessibility tree.
**Failure behavior.** A shell-side exception must not corrupt document truth (it holds none); it surfaces as an error event and, worst case, a UI-process restart that reopens documents from their files + sidecar journals (§10.2).
**Structure.** `bridge / app / chrome / canvas / panels / dialogs / a11y / platform` `[ADR-026]`.

## 2.2 Rust Core — top-level (Z0, UI process)

The core is a set of layered crates `[ADR-025]` composed by the `coordinator` crate. It is the trusted brain. Sub-components follow.

### 2.2.1 DocumentCoordinator (per open document)

**Responsibility.** Own the document's model + CoW overlay `[ADR-006]`, undo journal `[ADR-013]`, extraction/render scheduling for that document, and the lifecycle of its document worker. Serialize all mutations (single-writer invariant).
**Concurrency.** Actor-style: owned state + inbound command channel `[ADR-010]`. One logical task per document; no shared mutable document state across documents.
**Consumes.** Engine trait `[ADR-005]` via its worker; CacheGovernor budgets; Broker for privileged ops.
**Failure behavior.** If its worker dies, it respawns and replays (§10.1). If it panics, the panic is contained to that document's task; the supervisor logs, notifies the shell, and offers reopen-from-journal.

### 2.2.2 RenderScheduler

**Responsibility.** Convert viewport publications into prioritized tile requests, manage generation counters for cancellation, dispatch to the document worker, and route completed tiles to the CacheGovernor and shell (§6).
**Owns.** The pending-request set and priority queues per document. **Consumes.** Tile cache state; worker render capability.
**Failure behavior.** Stale requests are cheap to cancel (generation mismatch); worker loss reissues visible-tile requests only.

### 2.2.3 JobScheduler

**Responsibility.** Execute declarative jobs and job DAGs `[ADR-009]` on the utility pool with priority classes, progress, cooperative cancellation, and persistence for batch pipelines.
**Owns.** The job queue and persisted batch state. **Consumes.** Utility worker pool; Broker (for job I/O).
**Failure behavior.** A crashed utility worker fails its current job with a typed error; idempotent jobs auto-retry once; pipeline state persists across app restart.

### 2.2.4 CacheGovernor and MemoryGovernor

**Responsibility.** MemoryGovernor assigns and rebalances byte budgets across all caches from a global ceiling scaled to system RAM, and drives the degradation ladder under pressure `[ADR-011]`. CacheGovernor implements the weighted, revision-keyed caches (§8) and reports usage.
**Owns.** All cache storage and the accounting ledger (surfaced in diagnostics §2.2.10). **Failure behavior.** Under allocation failure, sheds cache before failing any user operation; never blocks the UI thread on reclamation.

### 2.2.5 UndoJournal (per document)

**Responsibility.** Maintain the append-only command-group log, produce/apply inverse deltas, persist the sidecar autosave journal, and reconcile with file revisions at save time `[ADR-013, ADR-021]`.
**Owns.** In-memory log + sidecar file handle. **Consumes.** The overlay (deltas apply there). **Failure behavior.** Sidecar write failure degrades to in-memory-only undo with a visible diagnostics warning (durability lost, editing continues).

### 2.2.6 Broker

**Responsibility.** The sole executor of privileged operations (file open/save, print spooling, clipboard, any network) on behalf of lower zones, with per-call validation `[ADR-016]`.
**Owns.** OS handles and capability grants. **Failure behavior.** Denies on validation failure with a typed reason surfaced to the user; never silently downgrades a privilege.

### 2.2.7 PluginHost — control plane (Z0)

**Responsibility.** Discover, verify, and manage lifecycle of plugins; hold capability grants; route plugin requests to the Broker/coordinator; enforce quotas. The *execution* plane lives in utility workers (Z2) `[ADR-014, ADR-015]`.
**Owns.** Plugin manifests, grant records, per-plugin quota accounting. **Failure behavior.** A misbehaving plugin instance is killed in its worker; the control plane revokes/reports without host impact.

### 2.2.8 Rendering subsystem (coordinator-side portions)

**Responsibility.** The Z0 half of §6: scheduling (2.2.2), tile-cache management (2.2.4/§8), and handing shared-memory tile handles to the shell. The *rasterization* itself is in Z1 (2.3).
**Note.** "Rendering" spans two zones by design; keep the split explicit in code and docs.

### 2.2.9 Search subsystem

**Responsibility.** Own the canonical per-page text model (from workers), serve in-document find, and manage the opt-in local cross-document index `[ADR-019]`.
**Owns.** Text-model cache (revision-keyed) and the Tantivy index (when enabled). **Consumes.** Extraction from workers; JobScheduler for indexing. **Failure behavior.** Unreliable extraction (ToUnicode pathology) is flagged, not silently searched wrong; index corruption triggers a rebuild job.

### 2.2.10 Diagnostics subsystem

**Responsibility.** Cross-process structured tracing with span propagation, the privacy-by-type redaction wrappers, the user-facing diagnostics panel data (leniency ledger, unsupported-feature log, memory ledger, worker restarts), and the inspector (COS/revision browser) `[ADR-020]`.
**Owns.** In-memory ring buffers; export assembly. **Failure behavior.** Diagnostics must never affect correctness; it is fail-open (drops traces under pressure rather than blocking).

### 2.2.11 Settings

**Responsibility.** Persist and serve user/UI preferences and enterprise policy overlays. **[SDS decision]** Two tiers: **UI settings** (shell-owned, e.g., panel layout, theme, shortcut-profile pin) stored via the platform's standard location; **behavioral policy** (core-owned, e.g., JS default, cache ceiling override, indexing enrollment, update channel) stored in a core-managed store readable by enterprise policy files (GPO/plist/`/etc`) `[ADR-030]`.
**Owns.** Effective-settings resolution (policy > user > default). **Failure behavior.** Corrupt settings fall back to defaults with a diagnostics note; policy files are read-only inputs and never rewritten.

## 2.3 Document Worker (Z1, one per document)

**Responsibility.** Host the engine backend (PDFium) behind the engine trait `[ADR-005]`; interpret content streams; rasterize tiles into shared memory; extract text-with-geometry; run the PDF-JS forms subset `[ADR-017]`. Compute only — holds no authoritative document state (the coordinator does).
**Concurrency.** Data-parallel work-stealing pool inside the process `[ADR-010]`; blocking is fine.
**Sandbox.** seccomp-bpf + namespaces (Linux), AppContainer + job object (Windows), Sandbox profile (macOS); no network; filesystem only via brokered handles `[ADR-016]`.
**Failure behavior.** Any crash is a coordinator event; the worker is respawnable and its state reconstructible from the file + overlay (§10.1). Workers are treated as disposable.

## 2.4 Utility Worker Pool (Z1, shared)

**Responsibility.** Host OCR (2.5), indexing extraction, optimization, thumbnail rendering, and the WASM plugin execution plane (2.7). Same sandbox profile as document workers; OS-level low priority so it never contends with interactive latency.
**Failure behavior.** Per-job isolation; a crash fails one job, not the pool.

## 2.5 OCR component

**Responsibility.** Implement the `OcrEngine` trait `[ADR-018]`: run the default Tesseract backend (vendored, sandboxed), execute the preprocessing→recognition→invisible-text-layer pipeline, and emit the normalized intermediate (text/boxes/confidence/orientation, with structure fields reserved for future auto-tagging).
**Owns.** Nothing persistent; produces Commands (add text layer) applied by the coordinator. **Failure behavior.** Recognition failure returns a typed low-confidence result; the coordinator surfaces "OCR uncertain" rather than writing garbage text.

## 2.6 Printing component **[SDS decision — placement]**

**Responsibility.** Print is split: **document preparation** (imposition, N-up, scaling, transparency flattening for legacy drivers) is a Z1 job producing a print-ready page sequence via our writer/engine; **spooling to the OS** is a Z0 Broker operation using Qt's `QPrinter`/`QPrintDialog` (native dialogs, driver integration) `[ADR-003]`. Rationale: preparation is untrusted content transformation (belongs in Z1); spooling touches OS device privileges (belongs in the Broker).
**Failure behavior.** Preparation failure aborts the print job with a reason; spooling failure surfaces the OS printer error verbatim.

## 2.7 Plugin Host — execution plane (Z2, in utility workers)

**Responsibility.** Instantiate WASM components (Wasmtime + Component Model, WIT interfaces `[ADR-015]`), enforce fuel/epoch CPU quotas and memory limits, mediate all host calls through the capability broker, and route document access through the same semantic API the app uses.
**Failure behavior.** Quota breach → preemption; fault → instance killed and reported; no plugin can reach a raw OS facility (double sandbox).

## 2.8 Signing component

**Responsibility.** PAdES B-B → B-LTA creation and validation `[ADR-016 futures, ADR-005]`: ByteRange hashing over the file, CMS/CAdES construction, PKCS#11/keychain/PFX key access (brokered), timestamp (RFC 3161) acquisition, DSS/VRI embedding, and DocMDP-aware incremental-update diff analysis for validation.
**Placement.** **[SDS decision]** Creation runs partly in Z0 (needs brokered key/HSM and network for timestamp/OCSP — all via Broker) and partly against the overlay/writer; validation's *diff analysis* runs in a minimal Z0 path but may move to a dedicated validation worker later. Signature bytes are computed over serialized output from `pdf-write` (§3.4) to guarantee the hashed bytes equal the saved bytes.
**Failure behavior.** Any ambiguity yields an explainable *indeterminate* result, never a false "valid" `[ADR-001 value 5]`.

## 2.9 Annotation component

**Responsibility.** Model the ~28 annotation types with **complete appearance streams always written** `[ADR-006 policy]`; attach text-markup to QuadPoints from the canonical text model; support FDF/XFDF import/export for interop; produce Commands for every change.
**Failure behavior.** On reading, prefer embedded appearances over synthesis; on writing, never emit an appearance-less annotation.

## 2.10 CLI

**Responsibility.** A headless client of the *same* coordinator/core as the GUI `[ADR-025]`, driving the command/event protocol and the job/pipeline system: convert, merge/split, OCR, optimize, redact-with-verification, sign, validate (veraPDF-style conformance), inspect (qpdf-style JSON). Proves the shell holds no privileged capability.
**Failure behavior.** Typed exit codes; machine-readable output mode; never interactive-only.

---

# 3. Document Lifecycle

This section is the authoritative sequence for each lifecycle transition. Steps are ordered; error branches reference §10.

## 3.1 Opening a document

1. **Intent.** Shell emits `OpenDocument{path | handle, open-options}`. Options include read-only hint, password (if pre-known), and recovery-scan preference.
2. **Broker.** Coordinator asks the Broker to open the path → an OS file handle validated for existence/permissions. The raw path never reaches Z1; only the handle does.
3. **Worker spawn.** Coordinator spawns a document worker (sandbox established *before* the handle is passed) and transfers the handle.
4. **Memory-map.** Worker mmaps the file (read-only). No bytes are eagerly read `[ADR-011]`.
5. **Bootstrap parse.** Worker locates the trailer, parses the xref (table or stream, following `/Prev`), and resolves the Catalog. **Encryption:** if an encryption dictionary is present, the worker reports "password required" (unless supplied); the coordinator brokers a password prompt (Z0 UI), derives the key in Z1, and proceeds. **Repair:** on xref failure, the worker runs reconstruction (full-file `N G obj` scan, qpdf-style) and records every deviation in the **leniency ledger**, returned to the coordinator for the diagnostics panel `[ADR-020]`.
6. **Model handshake.** Worker returns a *structural summary* (page count, page-tree shape, presence of AcroForm/XFA/JS/signatures/encryption, linearization status, leniency ledger). Coordinator constructs the Z0 document model: the COS store is fronted by the worker (objects materialized on request), while the coordinator holds the **CoW overlay** (initially empty) and the semantic façades `[ADR-006]`.
7. **XFA policy.** If XFA is detected, coordinator flags it (honest messaging; no XFA rendering) `[ADR-001 non-goal, ADR-017]`.
8. **Journal check.** Coordinator checks for a sidecar journal matching this file's identity (§10.3). If found → recovery offer (§3.6). If not → fresh session, new empty journal armed.
9. **First render.** Coordinator publishes an initial viewport (page 1 at fit scale) to the RenderScheduler (§6); progressive first paint follows.
10. **Ready event.** Coordinator emits `DocumentOpened{doc-id, summary}`; shell enables document UI, populates outline/thumbnail panels lazily (as jobs), and shows any leniency/XFA notices.

**Invariants.** Nothing in Z0 has parsed PDF syntax (the worker did). The original bytes are the immutable ground truth; the overlay is empty until the first edit.

## 3.2 Rendering (steady state)

Rendering is continuous, not a discrete lifecycle step; see §6 for the full pipeline. Lifecycle-relevant points: a render never mutates the model; render inputs are (revision, viewport, options); tiles are keyed by revision so an edit invalidates only affected pages (§8.1).

## 3.3 Editing

1. **Intent.** User action (annotate, fill field, insert/rotate/delete page, redact, edit image/text) → shell emits a domain command, e.g., `AddAnnotation{page, spec}`.
2. **Command construction.** Coordinator's semantic layer builds a **Command** `[ADR-013]`: named, parameterized, carrying its forward delta over the overlay and the information needed to invert it. Complex user operations compose child commands into one **group** (e.g., "Redact region" = remove-content + scrub-text-model + write-redaction-appearance).
3. **Apply.** The command applies its delta to the CoW overlay: new object versions are written keyed by the next revision; untouched objects are untouched (invariant). Semantic façades recompute lazily on next access (memoized, revision-keyed).
4. **Appearance/consistency pass.** For annotations/forms, the command regenerates appearance streams `[ADR-006 policy]`; for redaction, the verification substrate is prepared (§3.3.1).
5. **Journal.** The command group is appended to the UndoJournal and scheduled for sidecar persistence (batched, ≤ durability budget §10.3).
6. **Invalidate + re-render.** Coordinator bumps the document revision, invalidates affected pages' tiles and text-model entries, and republishes the current viewport → workers re-rasterize only what changed (§6, §8).
7. **Notify.** Coordinator emits `DocumentChanged{revision, affected-pages, dirty=true}`; shell updates title (dirty marker), enables undo, refreshes overlays.

### 3.3.1 Redaction (special path) `[opportunity O5]`

Redaction is a content-removal command, not a draw-black-box command: it rewrites affected content streams to delete covered glyphs/images, scrubs the canonical text model and any extraction caches, removes covered annotations, and clears relevant metadata/thumbnails. A **verification pass** (a paired job) re-extracts the *serialized* result and asserts absence of the redacted content, producing a signed report. Redaction cannot be "applied" as saved until verification passes; failure blocks the save with an explicit error.

### 3.3.2 Text editing (later phases) — lifecycle note

Text editing adds a content-stream micro-model (operators, text runs) under the same CoW/command discipline `[ADR-006 futures]`. The honesty rule binds here: if the embedded font subset lacks a needed glyph, the command surfaces "cannot edit safely — font subset incomplete" and offers substitution-with-embedding as an explicit choice, never a silent swap `[ADR-001 value 5]`.

## 3.4 Saving

1. **Intent.** `Save` (default) or `SaveAsClean` (explicit full rewrite).
2. **Default = incremental** `[ADR-012]`. `pdf-write` serializes the CoW overlay as: new/changed object versions → a new xref section (matching the file's existing xref style — table vs stream) → a trailer with `/Prev` to the previous xref. Untouched bytes are copied verbatim (or the original file is appended-to in place). Signatures over prior ByteRanges remain valid; post-signature edits must be DocMDP-legal or the save is refused with explanation.
3. **Signing interaction.** If the user is signing as part of this save (2.8), the writer reserves the `/Contents` hole, computes the ByteRange over the *actual serialized bytes*, obtains the CMS signature (brokered key + timestamp), and patches it in — guaranteeing hashed bytes == saved bytes.
4. **Atomicity.** Rename-path (temp + fsync + atomic rename) where supported; append-path (journal-intent record → append → commit) for locked/network files, enabling torn-write rollback via the intact `/Prev` chain (§10.4).
5. **Revision record.** A revision entry (id, timestamp, command-group summary) is recorded for the history timeline `[opportunity O6]`; the sidecar journal is reconciled (committed groups fold into the saved revision).
6. **Full-rewrite path** (`SaveAsClean`): linearize, repack object streams, garbage-collect unreferenced objects, optionally flatten history / sanitize metadata — always preceded by a **pre-flight report** enumerating what will be lost (signatures, history) and requiring confirmation.
7. **Notify.** `DocumentSaved{path, revision, mode}`; shell clears dirty state.

## 3.5 Closing

1. `CloseDocument{doc-id, discard?}`. If dirty and not discarding, coordinator prompts via shell.
2. On clean close: cancel in-flight jobs/renders for the doc, delete the sidecar journal (work is safely in the file), terminate the worker gracefully, return the document's cache budget to the MemoryGovernor, drop the coordinator task.
3. On discard-with-unsaved: same, but the sidecar journal is deleted only after confirming the user chose discard (otherwise it survives for recovery).
4. Other documents and the app are unaffected (independent fates).

## 3.6 Recovery

Triggered at open (3.1 step 8) when a sidecar journal exists for a file that was not cleanly closed. See §10.2 for the full algorithm. Summary: coordinator loads the original file, replays the persisted command groups against a fresh overlay, presents a **recovery summary** (named groups, timestamps) and lets the user restore or discard per document. Recovery is deterministic because commands are deltas over immutable original bytes.

---

# 4. Data Flow

All cross-boundary data flows through the typed protocol `[ADR-004]`. Bulk pixel data uses shared memory with handles passed through the protocol; nothing large is copied through the FFI or IPC serialization path.

## 4.1 Qt → Rust (shell → coordinator, in-process FFI)

**Mechanism.** The shell submits **commands** through the single `bridge/` surface; delivery is asynchronous with a correlation id `[ADR-004]`. The shell never blocks on the core.
**Payloads.** Small, typed (defined once in the `protocol` crate): navigation, viewport publications, domain edits, job submissions, settings changes, plugin actions.
**Threading.** Submitted from the Qt main thread; enqueued to the coordinator's inbox; the bridge returns immediately.
**Backpressure.** Viewport publications are *latest-wins* (coalesced by the scheduler); edit commands are never dropped.

## 4.2 Rust → Workers (coordinator → Z1, IPC + shared memory)

**Control.** Coordinator sends worker requests (render tile-set, extract page text, run JS event, run OCR job) over the IPC control channel — typed, correlation-id'd, cancellable via generation counters.
**Bulk out.** Input rarely needs bulk transfer (the worker already mmaps the file); when the coordinator must hand serialized overlay bytes to the engine (post-edit re-render, §3.3 step 6), it does so via shared memory, not the control channel.
**Sandbox note.** The worker cannot open files; any additional resource it needs is brokered by request back to Z0.

## 4.3 Workers → Rust (Z1 → coordinator)

**Bulk in (tiles).** Rasterized tiles are written into pre-negotiated shared-memory buffers; the worker sends a small `TileReady{gen, page, scale-bucket, tile-coord, shmem-handle, format}` control message. The coordinator validates the descriptor (bounds, format, generation) before use (Z1 output is untrusted, invariant 3).
**Structured results.** Text models, structural summaries, JS field-change requests, OCR intermediates, and leniency ledgers return as typed control messages, validated at the boundary.
**JS field changes.** A JS-initiated field mutation is not applied in Z1; the worker *requests* it, the coordinator turns it into a Command (undoable, attributable) `[ADR-017]`.

## 4.4 Rust → Qt (coordinator → shell)

**Mechanism.** The coordinator emits **events**; the bridge marshals them onto the Qt main thread via a queued dispatcher `[ADR-004]`.
**Event categories** (see §5): lifecycle (opened/saved/closed), change (revision/affected-pages), tiles-ready (with shmem handle for GPU upload), progress, notifications (leniency, unsupported-feature, worker-restart), errors, plugin-UI contributions.
**Tiles.** `TilesReady` carries validated shmem handles; the shell uploads to GPU textures and composites (§6.4). The shell maps, never copies through the bridge.

## 4.5 Plugin communication `[ADR-014, ADR-015]`

**Control plane (Z0).** PluginHost holds grants and mediates. **Execution plane (Z2).** Plugin instances run in utility workers.
**Flow.** Plugin calls a host function (WIT import) → intercepted by the capability broker in the worker → if the capability is granted and the call is document access, routed to the coordinator's semantic API (read snapshot, or submit a Command); if privileged (network/file), routed to the Z0 Broker. Results return through the same typed WIT boundary.
**UI.** Plugin panels are declarative schemas contributed to the shell via events; plugin code never executes in Z0. User input on a plugin panel becomes a plugin-action command routed to the instance.

## 4.6 Background jobs `[ADR-009]`

**Submission.** Any component (shell, CLI, plugin, coordinator) submits a job/DAG to the JobScheduler.
**Execution.** Dispatched to the utility pool under priority class; cooperative cancellation tokens; progress emitted as events (§5.4).
**Results.** Job outputs that mutate a document return as Commands to the owning coordinator (single-writer invariant); outputs that are files/reports go through the Broker.
**Persistence.** Batch pipeline state persists (§10) so long runs survive restart.

## 4.7 Flow summary (Informative)

```
User input ─▶ Shell ─(command,corr-id)▶ Coordinator ─(request,gen)▶ Worker(Z1)
                                             │                          │
   GPU◀─composite─ Shell ◀─(event/TilesReady,shmem)── Coordinator ◀─(TileReady,shmem)┘
                                             │
                        Broker(privileged) ◀─┤─▶ JobScheduler ─▶ Utility pool ─▶ (Commands/files)
                                             │
                        PluginHost(Z0) ◀────┴────▶ Plugin instance(Z2, in utility worker)
```

---

# 5. Event System

The event system is the runtime realization of the command/event protocol `[ADR-004]`. It is **not** a global publish-subscribe bus with anonymous subscribers (that pattern erodes ownership and defies the single-writer invariant); it is a set of **directed, typed channels** with defined producers and consumers. **[SDS decision]** Terminology and topology below.

## 5.1 Message taxonomy

- **Command** — an imperative from a client (shell/CLI/plugin) to the coordinator: "do this." Carries a correlation id. May be rejected (typed error) or accepted (produces events). Commands are the *only* way to change state.
- **Event** — a notification from the coordinator to clients: "this happened." Broadcast to interested clients of that document. Carries the causing correlation id when applicable.
- **Notification** — a subclass of event for user-facing advisories (leniency, unsupported feature, worker restart, memory pressure) surfaced in diagnostics/toasts.
- **Progress update** — a subclass of event tied to a job or long command, carrying (job-id, fraction, phase, cancellable?).
- **Domain Command (internal)** — the `[ADR-013]` Command object (undoable delta). Distinct from protocol Commands: a protocol `AddAnnotation` command *produces* an internal Command. Naming discipline: protocol Commands are verbs from clients; internal Commands are journaled deltas.

## 5.2 Channels and ownership

Each open document has: one **inbound command channel** (clients → its coordinator; single consumer = the actor, satisfying single-writer) and one **outbound event stream** (its coordinator → subscribed clients). Cross-document/global services (JobScheduler, PluginHost control, MemoryGovernor) have their own inbound command channels and emit into a **global event stream**. The bridge fans the global + per-document streams to the shell; the CLI subscribes to the same.

## 5.3 Cancellation

Two mechanisms, matched to cost:
1. **Generation counters** for renders/extraction: every request carries the viewport/revision generation; the worker checks it and abandons stale work cheaply; results with a stale generation are dropped at the coordinator boundary. This is the high-frequency path (scrolling produces constant supersession).
2. **Cancellation tokens** for jobs: cooperative, checked at safe points; a cancelled job unwinds, releases resources, and emits a terminal `JobCancelled` event. This is the coarse, user-initiated path.
Cancellation is always *cooperative and observable*; there is no forced thread kill inside a process (forced termination exists only at the *process* level for crash handling, §10).

## 5.4 Delivery guarantees and ordering **[SDS decision]**

- **Per-document event ordering is preserved** (the actor emits in causal order); clients may rely on it (e.g., `DocumentChanged` before the `TilesReady` for the new revision).
- **Commands are processed in submission order per client**, but the coordinator may reorder across clients by priority (edits over viewport publications) — never violating single-writer.
- **Latest-wins coalescing** applies to viewport publications and progress updates (a client only needs the newest); it never applies to edits, lifecycle, or terminal job events (delivered exactly once).
- **Backpressure:** if a client (shell) is slow to drain, coalesced streams collapse to newest; non-coalescible events queue with a bounded buffer whose overflow is a diagnostics-logged fault (should never occur in practice; if it does, it's a bug, not silent loss).

## 5.5 Error propagation

Every command may yield a typed `CommandError{corr-id, kind, explainable-reason}`. Errors are events, surfaced by the shell contextually (inline, toast, or dialog by severity). No error is swallowed; leniency (tolerated deviations) is a Notification, not an error — the distinction is deliberate `[ADR-020]`.

## 5.6 Why not a classic event bus (Informative rationale)

A global anonymous bus would let any component mutate or react to anything, defeating the single-writer invariant, obscuring data-flow for reviewers, and making cross-process ordering guarantees impossible. Directed typed channels keep producer/consumer explicit, keep the protocol as the one contract, and make the system analyzable — a ten-year-maintainability priority `[ADR-001]`.

---

# 6. Rendering Pipeline

Implements `[ADR-007]`. The pipeline is tile-based, pull-driven, multi-process, and revision-keyed. Stages below are ordered from user action to pixels.

## 6.1 Viewport publication

The canvas widget owns the *view state*: which pages are visible, their layout (continuous/single/facing), scroll offset, zoom scale, and rotation. On any change, it publishes a **viewport** to the RenderScheduler: a set of (page, visible-rect-in-page-space, scale, rotation) plus a **generation** counter. Publications are latest-wins (§5.4) — rapid scrolling produces one effective viewport, not a backlog.

## 6.2 Tile scheduling

The RenderScheduler decomposes the viewport into **device-space tiles** of a fixed size (**[SDS decision]** 256×256 logical px baseline, tuned per benchmark). For each tile it computes a key `(revision, page, scale-bucket, tile-coord, rotation)` (§8.1). It then:
1. Serves cache hits immediately (emit `TilesReady` for those).
2. For misses, assigns priority: **visible** > **prefetch ring** (a margin around the viewport for scroll-ahead) > **thumbnails** > **background** (index/thumbnail jobs).
3. Dispatches misses to the document worker with the current generation; superseded requests are cancelled by generation mismatch (§5.3).
Scale is **bucketed** (quantized zoom levels for cache reuse) with the exact user scale achieved by GPU sampling of the nearest bucket during transient zoom (§6.7).

## 6.3 Rasterization (Z1)

The document worker receives tile requests, invokes the engine trait to rasterize the requested page region at the requested scale into a **shared-memory buffer** (pre-negotiated pool, sized by the MemoryGovernor), and replies `TileReady{gen, key, shmem-handle, pixel-format}`. Rasterization uses the engine's Skia path where available `[ADR-005]`. The worker may hold an engine-side **display list** per page (record once, re-raster at multiple scales) as an optimization behind the trait — an `[ADR-007]`-sanctioned worker-side technique, invisible to Z0.

## 6.4 GPU upload and compositing (Z0)

The coordinator validates each `TileReady` descriptor (bounds/format/generation) and forwards `TilesReady` to the shell with the shmem handle. The canvas maps the buffer and uploads it as a GPU texture (**[SDS decision]** via Qt's RHI/QRhi abstraction so we ride Qt's Vulkan/Metal/D3D backends rather than committing to one API `[ADR-003]`). The compositor draws visible tiles into the viewport; missing tiles show the best available lower-scale content (§6.7) so there are no blank flashes. Text-selection and annotation overlays are drawn by the shell *on top*, from geometry (never baked into tiles), so selection is crisp at any zoom and independent of tile state.

## 6.5 Caching and 6.6 Invalidation

Tiles enter the byte-weighted LRU tile cache (§8.1), keyed as above, budgeted by the governor. **Invalidation is revision-keyed:** an edit bumps the document revision and marks affected pages; only tiles whose key's revision is now stale for a changed page are evicted — unaffected pages keep their tiles across edits. This is what makes editing a large document feel instant: annotate page 3, and pages 1–2 and 4–2000 never re-render.

## 6.7 Zoom, 6.8 Rotation, 6.9 Scrolling (interaction behaviors)

- **Zoom.** During a continuous zoom gesture, the GPU scales the nearest cached scale-bucket (instant, slightly soft); when the gesture settles, the scheduler requests crisp tiles at the new bucket. This decouples zoom smoothness from rasterization speed.
- **Rotation.** Rotation is part of the tile key; the worker rasterizes rotated tiles (engine handles the transform) so text/AA remain correct (GPU-rotating a tile would blur text). Transient rotation animates on the GPU against the old orientation until new tiles arrive.
- **Scrolling.** The prefetch ring keeps scroll-ahead tiles warm; velocity-aware prefetch (**[SDS decision]** widen the ring in the scroll direction proportional to velocity) hides latency on fast flicks. Overscroll into unrendered regions shows page-background placeholders with a subtle loading affordance, never blank white.

## 6.10 Progressive first paint

On open (or jump-to-page), the scheduler first requests a single low-scale whole-page raster (fast, one tile) for immediate content, then the crisp tile grid refines over it. First-page-visible latency is measured to the low-scale paint; crisp-complete is a secondary metric `[ADR-023]`.

## 6.11 Pipeline diagram (Informative)

```
Canvas view-state ─▶ [publish viewport+gen] ─▶ RenderScheduler
      ▲                                             │ decompose→tiles, key by (rev,page,scale,coord,rot)
      │ composite (GPU)                             ├─ cache hit ─▶ TilesReady ─┐
      │                                             └─ miss(prio,gen) ─▶ Worker(Z1) ─raster▶ shmem
      │                                                                              │
      └───────────── TilesReady(shmem handle) ◀── validate ◀── TileReady(gen,key) ◀─┘
Overlays (selection, annots) drawn by shell from geometry, above tiles.
```

---

# 7. Threading Model

Implements `[ADR-010]`. Three regimes, one membrane. The rule that prevents an entire class of bugs: **no lock is ever held across a channel send or an FFI/IPC call.**

## 7.1 UI thread (Z0, C++)

The Qt main thread runs the event loop and is the *only* shell thread. It: processes OS/input events, submits commands via the bridge (non-blocking), drains marshaled core events, uploads tiles, and paints. It never parses documents, never blocks on the core, and performs no file/network I/O directly (all brokered). Any shell work that could take >~a few ms is a core request. This keeps the UI at frame cadence regardless of document hostility.

## 7.2 Coordinator regime (Z0, Rust)

**Actor-per-document.** Each DocumentCoordinator is a single logical task owning that document's model, overlay, journal, and schedulers' per-doc state. It processes its inbound command channel sequentially — this *is* the single-writer guarantee, achieved without locks. Per-document tasks run on a small coordinator thread pool (**[SDS decision]** not one OS thread per document — tasks are multiplexed; a document idle-waiting on a worker doesn't hold a thread).

**Global services** (JobScheduler, PluginHost control, MemoryGovernor, CacheGovernor) are each their own owned-state task with a command channel. Cross-service interaction is by message, not shared state, with one sanctioned exception: the **tile cache index is a sharded concurrent structure** (read by many, guarded by fine-grained sharding) because the alternative — routing every cache lookup through a single task — would serialize rendering. This exception is explicitly reviewed and bounded `[ADR-010, ADR-027]`.

**No async runtime in the document core** `[ADR-010]`. Concurrency is threads + channels. The coordinator's "waiting" (on a worker reply) is modeled as the actor parking on its inbox for the correlated response, not as async/await coloring the API. Rationale reaffirmed: the public core API (used by CLI and embedders) must not be function-colored.

## 7.3 Worker threads (Z1, Rust)

Each worker process runs a **work-stealing compute pool** (rayon-class) sized to its core budget. Rasterization of independent tiles, multi-page extraction, and OCR sub-steps parallelize here. Blocking is acceptable — a worker is a compute process. Workers hold no cross-process locks; they own their mmap and engine instance. The IPC handler runs on a dedicated thread that dispatches to the pool and returns results, so a long raster never blocks incoming cancellations.

## 7.4 Synchronization and ownership rules (Normative)

1. Document truth is owned by exactly one coordinator task; access is by message.
2. Shared concurrent structures are the rare, reviewed exception (tile-cache index, memory ledger counters) and use lock-free or sharded designs, never coarse mutexes on hot paths.
3. Locks (where unavoidable) are leaf-level, never held across sends/FFI/IPC, and lock-order is documented where more than one exists.
4. Every thread and task has a stable name surfaced in diagnostics `[ADR-020]`.
5. The protocol boundary is the only place regimes meet; each side validates the other's messages.

## 7.5 Determinism for testing

Because coordinators are message-driven with ordered per-document inboxes, a recorded inbox sequence replays deterministically (workers stubbed) — the basis for coordinator-level regression and fault-injection tests `[ADR-022]`.

---

# 8. Cache Design

Implements `[ADR-011]` caches. All caches are: byte-weighted, centrally budgeted by the MemoryGovernor, revision-aware where they hold document-derived data, and individually inspectable in diagnostics. **[SDS decision]** Each cache below specifies key, value, weight, invalidation, and scope.

## 8.1 Tile cache

- **Key:** `(doc-id, revision, page, scale-bucket, tile-coord, rotation)`.
- **Value:** rasterized tile (in a shared-memory-backed slab) + metadata.
- **Weight:** exact byte size of the pixel buffer.
- **Invalidation:** revision-keyed — on edit, only stale-revision tiles of *changed* pages are evicted; scale-bucket eviction under memory pressure prefers off-screen and non-current buckets.
- **Scope:** per-document budget within the global ceiling; returned to the pool on close.
- **Note:** the single largest consumer; first to shed under pressure (degradation ladder §9.3).

## 8.2 Object cache (decoded COS objects / streams)

- **Key:** `(doc-id, object-id, revision-resolved)`.
- **Value:** decoded object (decompressed stream, parsed dictionary) — this is *cache*, the mmap'd bytes are the state `[ADR-011]`.
- **Weight:** decoded byte size.
- **Invalidation:** an object version is immutable; a new revision creates a new key, old versions evict by LRU. Overlay objects (unsaved edits) are pinned (not evictable) until saved or undone.
- **Scope:** per-document. Lives partly in the worker (engine's own object handling) and partly in the coordinator (overlay objects); the coordinator's is authoritative for edited objects.

## 8.3 Font cache

- **Key:** `(font-program-hash, size/hinting params)` — hashed by content so identical embedded fonts across documents share.
- **Value:** parsed/sanitized font program + rasterized glyph atlas entries.
- **Weight:** glyph atlas bytes + parsed tables.
- **Invalidation:** content-hash keyed → effectively immutable; pure LRU. **Cross-document sharing** is a deliberate win (many PDFs embed the same standard fonts).
- **Scope:** global (shared across documents), since keyed by content, not doc-id. Lives in workers (rasterization) with a coordinator-side registry for accounting.

## 8.4 Image cache (decoded image XObjects)

- **Key:** `(doc-id, object-id, revision, target-scale-bucket)`.
- **Value:** decoded (and possibly downsampled) image ready for compositing into tiles.
- **Weight:** decoded byte size.
- **Invalidation:** revision + object; downsampled variants evict before full-res under pressure.
- **Scope:** per-document. Large scanned-image pages make this significant; the governor caps it and prefers on-demand re-decode over retention when pressure is high.

## 8.5 Search cache (canonical text model)

- **Key:** `(doc-id, page, revision)`.
- **Value:** the canonical per-page text model (Unicode runs + geometry + reading order + reliability flag) `[ADR-019]`.
- **Weight:** model byte size.
- **Invalidation:** revision + page (edits to a page invalidate its text model, triggering re-extraction on next need).
- **Scope:** per-document; shared by find, selection, accessibility export, compare, and index feeding — the single-extraction invariant.

## 8.6 Thumbnail cache

- **Key:** `(doc-id, page, revision, thumb-size)`.
- **Value:** small whole-page raster.
- **Weight:** pixel bytes.
- **Invalidation:** revision + page; lowest priority — first to evict, cheapest to regenerate (background job).
- **Scope:** per-document, with an optional persistent on-disk thumbnail store for the recent-files UI (**[SDS decision]** opt-in, in app-state, revision-fingerprinted, honoring the no-surprise-disk-use value `[ADR-016]`).

## 8.7 Governance across caches

The MemoryGovernor holds a live ledger of every cache's current bytes vs. budget. Budgets are proportions of the global ceiling, themselves scaled to system RAM with a hard floor for correctness (enough to render the current viewport at current zoom). Under pressure the governor rebalances (shrinks low-priority caches first, §9.3). The diagnostics panel renders this ledger live `[ADR-020]` — caches are never a black box.

---

# 9. Memory Lifecycle

Implements `[ADR-011]`. This section states *when* memory is allocated, retained, and freed, end to end.

## 9.1 Allocation

- **File bytes:** memory-mapped on open; the OS pages them in on access. We never read the whole file into heap.
- **Decoded data (objects/streams/images/fonts/text/tiles):** allocated on demand into the appropriate governed cache; treated as reconstructible cache, not state.
- **Overlay (edits):** allocated per Command as new object versions in the CoW overlay; this is *state*, not cache — pinned until saved or undone.
- **Per-render scratch:** allocated into an **arena** in the worker for the duration of one page/tile render, then dropped wholesale `[ADR-011]` — bounds fragmentation and makes per-render cost measurable.
- **Shared-memory tile buffers:** allocated from pre-negotiated pools sized by the governor; recycled, not per-tile-malloc'd.

## 9.2 Retention and freeing (deterministic points)

- **Cache entries** free on eviction (LRU under budget) or on revision invalidation (§8). Freeing is deterministic given the policy, not GC-timing-dependent (Rust ownership) `[ADR-002]`.
- **Overlay objects** free when: the Command is undone (delta reverted) or the document is saved (folded into the file; the in-memory version may then become ordinary evictable cache) or the document closes.
- **Worker memory** frees entirely on worker termination — a crashed/closed document returns its full footprint immediately (a reliability *and* memory benefit of the process model `[ADR-008]`).
- **Arenas** free at render completion.
- **A document's entire budget** returns to the global pool on close (§3.5).

## 9.3 Eviction and the degradation ladder

Under memory pressure (governor threshold or OS pressure signal), in order `[ADR-011]`:
1. Drop the prefetch ring (keep only strictly visible tiles).
2. Downsample/evict off-screen and non-current scale-bucket tiles.
3. Shrink decoded object/image caches (re-decode on demand).
4. Drop thumbnail and search caches for background documents.
5. **Last resort:** collapse to low-memory shared-worker mode (multiple documents share a worker; loses per-document crash isolation — a documented, temporary trade `[ADR-008 alt-c]`) and warn in diagnostics.
The UI thread is *never* blocked on reclamation; eviction runs on governor tasks. The system degrades to slower, never to crashed.

## 9.4 Leak discipline

Any container growing with document size or session length must declare a bound/eviction policy or fail review `[ADR-011, ADR-027]`. The memory ledger makes growth observable; long-run soak tests (open/close/scroll/edit for hours) are part of benchmarking `[ADR-023]` and assert a stable steady state.

---

# 10. Error Recovery

Implements `[ADR-021]`. The guarantee: **no single failure — worker crash, app crash, or torn save — loses more than the durability budget of committed work** (**[SDS decision]** ≤ 2 seconds or N commands, whichever first).

## 10.1 Worker crash

Detected by the coordinator via IPC channel death / process-exit signal.
1. Coordinator marks the document's worker dead; in-flight render/extract requests are considered failed (their generations are abandoned).
2. Coordinator respawns a worker (fresh sandbox), re-transfers the brokered file handle, and re-runs the bootstrap parse.
3. **State reconstruction:** the document's authoritative state is (original file bytes) + (CoW overlay held in Z0) + (journal). The worker never held authoritative state, so nothing is lost; the coordinator re-applies the current overlay to the re-opened engine view for rendering.
4. Visible-tile requests reissue at the current generation; the user sees a brief re-render, optionally a subtle notice, and a diagnostics entry. Repeated crashes on the same document (e.g., a pathological page) trip a circuit breaker: that page renders as a placeholder with an explicit "cannot render" note, the rest of the document remains usable `[ADR-001 value 5]`.

## 10.2 App (coordinator/UI process) crash and recovery

1. **During normal operation**, each mutating Command group is persisted to the document's **sidecar journal** within the durability budget (§10.3).
2. **On crash**, the process dies; sidecar journals for not-cleanly-closed documents remain on disk.
3. **On next launch**, the coordinator scans the app-state journal directory. For each orphaned journal it resolves the source file (§10.3 identity), and offers recovery.
4. **Replay:** open the original file → create a fresh empty overlay → replay the journal's command groups in order against it (deterministic: commands are deltas over immutable original bytes) → arrive at the pre-crash overlay state.
5. **Present** a recovery summary per document (named command groups + timestamps) with restore/discard choice. Restored documents are dirty (unsaved), exactly as before the crash.
6. Crash artifacts (minidumps) are captured locally; submission is user-initiated and shows contents first `[ADR-020, ADR-016]`.

## 10.3 Autosave (journal persistence) details

- **What:** the command journal (deltas), not full-document copies — O(change) I/O, no racing on large files `[ADR-021]` (contrast with incumbent full-file autosave).
- **When:** committed within the durability budget of any mutation (batched to avoid I/O per keystroke; a burst of edits flushes as one group set).
- **Where:** per-user app-state directory, **never** beside the source document (avoids litter and leaking work-in-progress into shared/VCS folders).
- **Identity:** each journal records a robust source-file fingerprint (path + size + mtime + content hash of key structural bytes) so recovery re-associates even if the file moved, and detects if the underlying file changed since (in which case recovery warns rather than blindly replaying).
- **Privacy:** if the source document is encrypted, the sidecar journal is encrypted with a key derived from the session/document credentials; journals are deleted on clean save/close.

## 10.4 Document corruption (at open)

Handled by the worker's repair path (§3.1 step 5): xref reconstruction, object-stream salvage, best-effort Catalog/page-tree recovery. Every tolerated deviation is recorded in the **leniency ledger** and shown to the user (honest: "this file was damaged; here is what we repaired") `[ADR-020]`. Unrecoverable files fail with a specific diagnosis, not a generic error. A "salvage export" job can write out whatever content was recoverable.

## 10.5 Torn save recovery

Per §3.4 step 4: rename-path saves are atomic (no torn state possible). Append-path saves write a journal-intent record before appending; if the process dies mid-append, next open detects the incomplete increment and truncates back to the last valid xref (the `/Prev` chain guarantees the prior revision is wholly intact) — the file always opens as *some* valid revision, never a corrupt hybrid.

## 10.6 Recovery testing

Fault injection is a first-class test stratum `[ADR-022]`: scripted worker kills mid-render, coordinator kills mid-mutation (asserting ≤ budget loss on replay), simulated torn appends (asserting truncation to a valid revision), and corrupt-file corpora (asserting graceful leniency, never a crash).

---

# 11. Plugin Lifecycle

Implements `[ADR-014, ADR-015]`. Plugins are WASM components (Wasmtime + Component Model, WIT interfaces) running in utility workers (Z2), double-sandboxed inside the OS sandbox (Z1).

## 11.1 Discovery and verification

Plugins are packaged with a **manifest** (identity, version, required capabilities, contributed UI schemas, WIT world targeted). On discovery, the PluginHost (Z0) verifies package integrity and, when a signed registry exists, signature and provenance `[ADR-014 futures]`. Manifests declaring capabilities the current app version's WIT world does not provide are rejected with a clear version-mismatch reason.

## 11.2 Loading and initialization

1. User (or policy) enables a plugin. PluginHost records the enablement and the **granted capability set** (see 11.3).
2. On first use (lazy) or eager (if configured), PluginHost asks a utility worker to instantiate the component: Wasmtime compiles/links the module against the WIT world, injecting only host functions for granted capabilities — **undeclared capabilities are unlinkable, not merely denied** `[ADR-014]`.
3. The instance receives an init call; it registers job types, tools, and panel schemas, which flow as events to the shell.
4. CPU quota (fuel/epoch) and memory limits are set on the store before any guest code runs.

## 11.3 Permissions (capability model)

Capabilities are explicit and least-privilege: e.g., `read-text`, `read-structure`, `annotate` (submit annotation Commands), `register-job`, `contribute-panel`, `network` (brokered), `read-file`/`write-file` (brokered, scoped). The user grants per-capability at enable time with human-readable descriptions; grants are revocable. A plugin can never exceed its grant because ungranted host functions are absent from its instance. Document mutation is only ever via Commands (undoable, attributable to the plugin) — there is no direct-write capability.

## 11.4 Communication (runtime)

Guest → host calls cross the typed WIT boundary into the worker's capability broker: document reads return snapshots from the coordinator's semantic API; mutations submit Commands to the owning coordinator; privileged ops route to the Z0 Broker. Host → guest calls invoke plugin-registered handlers (e.g., a job execution, a tool action). All calls are typed; large data uses component-model resource handles / streams, not giant buffers. Plugins are preemptable: exceeding CPU fuel yields an interruption; exceeding memory limits faults the instance.

## 11.5 Termination

- **Graceful:** on disable/close, the host calls the guest shutdown hook, drains outstanding calls, and drops the instance; contributed UI is retracted via events.
- **Forced:** on quota breach or fault, the instance is killed immediately in its worker; the control plane records the reason, retracts UI, and may disable the plugin after repeated faults (circuit breaker). No plugin fault propagates to Z0 or to documents (its in-flight Commands either committed atomically or not at all).
- **Isolation of blast radius:** because plugins run in the shared utility pool, a plugin crash may disturb *other jobs in that worker*; the scheduler re-runs affected idempotent jobs. **[SDS decision]** High-risk or untrusted plugins can be pinned to a dedicated single-plugin worker (policy/registry-driven) for full isolation at higher memory cost.

## 11.6 Versioning and compatibility

The WIT world is the semver unit `[ADR-015, ADR-030]`; deprecated interfaces coexist with successors for ≥ 2 release trains. The SDK ships a compatibility test-kit so authors verify against a target world in CI. A plugin pins the world it needs; the host offers all still-supported worlds.

---

# 12. Security Model

Implements `[ADR-016]`. This section is the consolidated, normative security view; other sections reference it.

## 12.1 Trust boundaries (zones)

- **Z0 (trusted):** UI process — Qt shell + Rust coordinator + Broker + PluginHost control. Minimal parsing; owns privileges and document truth.
- **Z1 (untrusted compute):** document workers and utility pool — engine, interpretation, extraction, OCR, plugin execution host. Sandboxed; no network; brokered file access only.
- **Z2 (untrusted plugin):** WASM instances inside Z1 — capability-scoped, unlinkable beyond grants.
- **Z3 (adversarial data):** the document itself — never trusted, always validated.
Data crossing *upward* (Z1→Z0, Z2→Z1) is validated at the boundary (invariant 3). Privileges flow *downward* only via brokered request/execute.

## 12.2 Sandbox (per platform)

- **Linux:** seccomp-bpf syscall filter + user/mount/pid namespaces; no network namespace access; filesystem via passed handles only.
- **Windows:** AppContainer with minimal capabilities + a restricted job object; brokered handles.
- **macOS:** Sandbox profile (`sandbox_init`-class) denying network and filesystem except brokered handles.
Sandboxes are established *before* any document handle or guest code enters the process (§3.1 step 3). Sandbox escape is a release-blocking severity `[ADR-016]`.

## 12.3 Capability model

Every privileged action is a named capability executed by the Z0 Broker after per-call validation. Lower zones possess *requests*, never handles. Plugin capabilities (11.3) are the Z2 refinement of the same principle. The Broker's catalog is the authoritative, documented list of everything the app can do to the outside world — doubling as the privilege audit surface.

## 12.4 FFI safety (the bridge)

The Rust↔Qt bridge `[ADR-004]` is the one in-process language boundary and receives the strictest discipline `[ADR-027]`: `cxx`-checked signatures (no hand-rolled ABI), no exceptions crossing the boundary, no raw pointers owned across it, payloads validated on receipt, and mandatory two-reviewer changes. It carries commands/events/handle-descriptors only — never document objects — limiting what a bug there can corrupt.

## 12.5 Plugin safety

Double isolation (WASM sandbox within OS sandbox), unlinkable-beyond-grant capabilities, mutation-only-via-Command, CPU/memory quotas with preemption, and optional dedicated-worker pinning (11.5). A malicious plugin's maximum impact is: consume its quota (then preempted), disturb co-scheduled idempotent jobs (re-run), or attempt brokered calls (validated/denied). It cannot reach the network or filesystem except through granted, brokered, scoped capabilities; it cannot touch Z0 memory or other documents' truth.

## 12.6 Content-driven attack surface

- **JavaScript** runs only in Z1, forms-subset only, with zero broker capabilities `[ADR-017]`.
- **Actions:** URI actions require visible consent with full URL; `/Launch`/embedded-executable actions are refused unconditionally; remote resource loading is off by default, per-document opt-in `[ADR-016]`.
- **Filters/fonts/parsers:** the historical CVE cluster — all in Z1, all continuously fuzzed `[ADR-022]`, all memory-safe Rust in our own code (the C/C++ engine mass is confined to Z1 behind the trait).
- **Signatures:** validation never yields a false "valid"; DocMDP diff analysis runs on the incremental-update history to detect illegal post-signature changes (2.8).

## 12.7 Process and disclosure

Continuous fuzzing + OSS-Fuzz enrollment at public launch; published security policy and private disclosure channel with a CVE-handling SLA; annual external audit of the Broker + bridge + sandbox; reproducible builds so a shipped binary is verifiably the audited source `[ADR-029]`. No default telemetry — security posture is proven by construction and audit, not by phone-home `[ADR-020]`.

---

# 13. Build System

Implements `[ADR-024, ADR-025, ADR-026, ADR-028, ADR-029]`. The build must handle two languages, three platforms, a Chromium-style vendored engine, WASM SDK toolchains, and reproducibility — while keeping contributor onboarding to roughly one clone plus one artifact fetch.

## 13.1 Overall structure

The primary monorepo `[ADR-024]` contains `core/` (Cargo workspace), `shell/` (CMake/Qt), `cli/`, `plugin-sdk/` (WIT + bindings + test-kit), `docs/`, `tools/`, and `third_party/` (vendored engines with provenance manifests). Two build subsystems meet at the FFI bridge.

## 13.2 Cargo workspace (`core/`)

Built with a pinned stable Rust toolchain (MSRV policy: trailing ~2 stable versions `[ADR-027]`), lockfile-exact `[ADR-028]`. Feature flags gate engine backends (`engine-pdfium`, `engine-hayro`), scripting (V8 vs. leaner interpreter), and per-OS sandbox implementations `[ADR-025]`. The workspace produces: the coordinator/core libraries, the Z1 `worker-main` binary, the `cli` binary, and the `ffi-bridge` static library consumed by the shell. `cxx` generates the bridge headers/sources during the Cargo build for CMake to consume.

## 13.3 CMake + Qt (`shell/`)

The shell is a CMake project targeting Qt 6 Widgets, linking the `ffi-bridge` static library and its generated headers. **[SDS decision]** Cargo is the driver for core artifacts; CMake consumes them via a thin integration (a generated CMake package describing the bridge lib + include dirs), invoked as a pre-build step. This avoids a bidirectional build dependency (CMake calling Cargo calling CMake) — the direction is always Cargo-first, then CMake links. Qt is dynamically linked (LGPL compliance `[ADR-003, ADR-028]`). The shell build produces the final application bundle per platform.

## 13.4 Vendored engine (`third_party/`)

PDFium is vendored with a pinned upstream ref and a maintained patch series `[ADR-028]`. Because its native build (GN/depot_tools) is heavy and hostile to contributors `[ADR-005]`, the project provides **prebuilt engine artifacts** per platform/arch, fetched by a setup step; the full engine build runs only in CI when `third_party/` changes, or for contributors who opt in. This is the single most important contributor-friction mitigation in the build `[ADR-029]`.

## 13.5 WASM SDK toolchain (`plugin-sdk/`)

WIT worlds are the source of truth; the SDK generates guest bindings (Rust first; JS/TS, Python, Go as toolchains mature `[ADR-015]`) and ships the compatibility test-kit. Building a plugin does not require building the app — the SDK depends only on the published WIT worlds and a WASM target toolchain.

## 13.6 Cross-platform builds

Native builds on each OS produce native artifacts (no cross-compilation of the Qt shell in the baseline — native toolchains give correct platform integration and dialogs). Per platform: Windows (MSVC toolchain, AppContainer manifest, code signing, MSI/MSIX for enterprise), macOS (clang, Sandbox entitlements, notarization, PKG/DMG), Linux (system or pinned toolchain, seccomp/namespace setup, repo packages + Flatpak) `[ADR-030]`. Platform-specific sandbox and shell-integration code lives in `core/sandbox` and `shell/platform` respectively, selected by build config.

## 13.7 CI/CD

Per `[ADR-029]`: path-aware PR pipeline (format/lint both languages → affected-crate tests → protocol/WIT compat check → corpus subset with image-diff artifacts → shell QTest), targeting ≤ 20 min via prebuilt engines and caching. Merge pipeline runs full corpus × 3 OSes, differential-oracle dashboards, fuzz smoke, fault-injection, and non-gating benchmark trends. Release pipeline adds hard benchmark gates on dedicated hardware, veraPDF conformance of writer output, the interop matrix, **reproducible-build verification** (two independent builders → bit-identical artifacts, divergence blocks release), signed artifacts with SLSA-style provenance, and SBOM publication. Continuous fuzzing runs off-pipeline (OSS-Fuzz).

## 13.8 Reproducibility

Reproducible builds `[ADR-016, ADR-029]` require deterministic inputs: pinned toolchains, lockfiles, vendored engine at a fixed ref built with a pinned config, normalized build paths and timestamps, and stable link order. This is genuine engineering effort (PDFium's build is the hard part) and is treated as a standing obligation, not a one-time setup — it underwrites the "trust by construction" value with a verifiable claim.

---

# 14. Development Roadmap

Implements the phasing of `[ADR-001]` and the capability map (research Stage 1). **Principle:** every milestone ends with a *shippable, independently testable application* — never a half-integrated substrate. Foundations that produce no user-visible feature are still delivered as *working tools* (CLI/inspector) so they are exercised and testable. Sequencing respects the dependency spine: overlay/model → save/undo/recovery → rendering → features.

Milestones are numbered M0–M12 across roughly five years. Each lists **goal**, **user-visible deliverable**, **exit criteria** (testable), and **key risks**. Durations are indicative, not commitments.

## M0 — Foundations and walking skeleton (the riskiest integration, done first)
**Goal.** Prove the whole architecture end-to-end on the narrowest possible slice.
**Deliverable.** An app that opens a simple PDF and renders page 1 — via the real multi-process pipeline: shell → coordinator → sandboxed worker → PDFium → shared-memory tile → GPU composite. Plus a CLI that opens and reports structural summary.
**Exit criteria.** A tile rendered through the real bridge + IPC + shmem path on all 3 OSes; worker runs sandboxed; the corpus-diff harness, benchmark harness, and CI pipeline exist and gate this slice; kill-the-worker test shows transparent respawn. Cold-start and first-page budgets measured (baseline recorded).
**Risks.** Bridge + shared-memory + sandbox are the hardest plumbing; doing them first de-risks everything. This milestone is deliberately feature-poor and infrastructure-rich.

## M1 — Robust viewer
**Goal.** A genuinely good read-only viewer.
**Deliverable.** Full tiled rendering (zoom/rotate/scroll/prefetch §6), multi-page layouts, outline/thumbnail/attachment/layer (read) panels, encrypted-file open, file repair with leniency ledger + diagnostics panel, accessible UI surface (screen-reader navigation of chrome).
**Exit criteria.** Smooth-scroll and large-document (2,000-page, scan-heavy) benchmarks pass budgets at p95; corpus render pass-rate ≥ target vs. PDFium oracle; repair corpus opens without crashes; accessibility audit of chrome passes.
**Risks.** Large-document performance and the repair long tail. This is the SumatraPDF-class wedge — ship it excellent.

## M2 — Text: selection, search, extraction
**Goal.** The canonical text model and its consumers.
**Deliverable.** Text selection, in-document find (normalized, instant-first-hit), copy, text export; ToUnicode-pathology honesty flags; accessibility text export of tagged reading order where present.
**Exit criteria.** Extraction correctness suite (ligatures, RTL, CJK, hyphenation) passes; find latency budget met on large docs; unreliable-extraction flagging verified on known-bad corpus.
**Risks.** Extraction correctness is a long tail; the reliability flag keeps us honest where it fails.

## M3 — Mutation core: save, undo, recovery
**Goal.** The write spine, proven before any rich editing feature.
**Deliverable.** CoW overlay + Command/journal fully wired; incremental save `[ADR-012]`; unlimited undo/redo; sidecar autosave + crash recovery + torn-save rollback (§10); the simplest real edit (page organize: reorder/rotate/delete/insert) to exercise it end-to-end; revision-history timeline (read) `[opportunity O6]`.
**Exit criteria.** Fault-injection suite passes (≤ durability-budget loss on app-kill; valid-revision guarantee on torn append); incremental saves preserve untouched bytes (byte-diff test) and existing signatures (validation test); undo across a crash restores state.
**Risks.** This milestone is where correctness matters most; it gates every subsequent feature. No editing feature ships before this is solid.

## M4 — Annotation and commenting
**Goal.** The most common professional workflow after reading.
**Deliverable.** Core annotation types with always-written appearance streams; text-markup via QuadPoints; sticky notes/ink/shapes/stamps/free-text; comment threading + review status; FDF/XFDF import/export for Acrobat interop `[ADR-...]`.
**Exit criteria.** Interop matrix passes (annotations authored here render correctly in Acrobat/Foxit/browsers and vice versa); every annotation edit is undoable; ink latency budget met.
**Risks.** Appearance-stream fidelity and XFDF interop — tested against the incumbents, not just the spec.

## M5 — Forms (fill) + JavaScript forms subset
**Goal.** "The form just works."
**Deliverable.** AcroForm fill with appearance regeneration; the PDF-JS forms subset in Z1 `[ADR-017]` (validation/calculation/format, calc order); FDF/XFDF data round-trip; per-document JS indicator + kill switch + compatibility logging.
**Exit criteria.** A corpus of real enterprise forms computes correctly; unsupported JS no-ops are logged, never silently mis-emulated; JS runs only in the sandboxed worker with zero broker reach (verified).
**Risks.** The JS subset boundary will generate compatibility reports; the living compatibility table manages expectations.

## M6 — Assembly toolkit + batch + CLI parity
**Goal.** The Family-D workhorse set and the headless ecosystem position `[opportunity O8]`.
**Deliverable.** Merge/split (resource-correct, no bloat), compress/optimize (with pre-flight loss report), watermarks/headers-footers/Bates, the job-DAG batch UI, and full CLI parity for all of it.
**Exit criteria.** Merge-without-bloat verified (object-dedup test); optimize preserves fidelity/tags unless explicitly dropped; every GUI operation reproducible via CLI pipeline (parity test).
**Risks.** Structural correctness (qpdf-class); inheritance/resource-sharing edge cases.

## M7 — Redaction (provable)
**Goal.** The legal-market wedge done right `[opportunity O5]`.
**Deliverable.** Content-removal redaction (streams/text-model/metadata/annotations/thumbnails scrubbed) with the mandatory verification pass + signed report; block-save-until-verified.
**Exit criteria.** Verification re-extraction proves absence on the redaction corpus; no cosmetic-only path exists; metadata/history scrubbing verified.
**Risks.** Completeness of removal across all content forms; the verification pass is the safety net and the differentiator.

## M8 — Signatures (validation + software signing)
**Goal.** The signature-trust wedge `[opportunity O4]`.
**Deliverable.** PAdES validation with explainable results + DocMDP diff analysis; software-certificate signing (PFX/keychain, brokered); RFC-3161 timestamps; DSS/VRI embedding (B-T, B-LT).
**Exit criteria.** Validation matches reference (pyHanko/DSS) on a signature corpus incl. tampered files (never false-valid); signed output validates in Acrobat; hashed bytes == saved bytes (incremental-save integration test).
**Risks.** Crypto lifecycle correctness; validation must be conservative (indeterminate over false-valid).

## M9 — OCR + searchable scans
**Goal.** Scanned-document workflows `[ADR-018]`.
**Deliverable.** Tesseract backend (sandboxed) + preprocessing pipeline + correct invisible-text-layer registration + per-page skip logic (fix Acrobat's "renderable text" refusal) + PDF/A-conform output option; batch + CLI.
**Exit criteria.** Text-layer registration accuracy on the scan corpus; searchable output verified; JBIG2 symbol-mode off by default (hazard warning present).
**Risks.** Preprocessing quality; engine-plural boundary keeps recognition swappable.

## M10 — Hardware signing (PAdES-LTA) + compliance validation
**Goal.** Regulated-industry completeness.
**Deliverable.** PKCS#11 hardware-token/HSM signing; B-LTA (document timestamps + archival validation data); PDF/A validation (veraPDF-class) integrated; PDF/A export.
**Exit criteria.** Hardware-token signing on reference devices; LTA validity survives simulated certificate expiry; PDF/A export passes veraPDF.
**Risks.** Hardware/device variability; signature-length prediction with tokens.

## M11 — Plugin system (public)
**Goal.** Open the extensibility ecosystem `[ADR-014, ADR-015]`.
**Deliverable.** Public WASM/WIT plugin runtime, capability model + grant UI, declarative panel/tool/job contributions, SDK + compatibility test-kit + docs; a few first-party plugins dogfooding the API.
**Exit criteria.** A malicious-plugin test is contained (quota/preemption/no-escape verified); WIT semver compat checks gate CI; a third-party author can build/ship against a stable world using only published SDK.
**Risks.** API surface stability — designed conservatively, versioned per-contract.

## M12 — Content editing (image, then text) + compare
**Goal.** Acrobat's crown-jewel territory, entered last and carefully `[ADR-006 futures]`.
**Deliverable.** Image/object editing (move/resize/replace) first; then text editing with the content-stream micro-model, layout inference, and honest font-subset handling (edit-safely-or-say-so); document compare (visual + semantic text diff) `[opportunity]`.
**Exit criteria.** Edits are undoable and incremental-save-clean (no transcoding, byte-diff verified); font-subset-incomplete cases surface honestly (never silent substitution); compare accuracy on a versioned-document corpus.
**Risks.** Text editing is the hardest capability in the product; it deserves its own design spike before M12 begins, and may itself split into sub-milestones.

## Beyond M12 (Informative)
PDF/UA remediation tooling with ML-assisted auto-tagging `[opportunity O7]`; PDF/X prepress color pipeline; Office export (studied integration, not from-scratch); self-hostable collaboration server (only under a superseding ADR to `[ADR-001]`'s cloud non-goal). Each is a multi-milestone program in its own right.

## Roadmap invariants (Normative)
1. No editing feature (M4+) ships before the mutation core (M3) passes its fault-injection gate.
2. Every milestone updates the corpus, interop matrix, and benchmark baselines; regressions block the milestone.
3. Every milestone's features are reachable from both GUI and CLI where meaningful (parity is a permanent property, not a phase).
4. Each milestone is a releasable build on the standard channels `[ADR-030]`; there are no "integration-only" releases hidden from users.

---

*End of System Design Specification (baseline). This document is maintained alongside the code; material changes to any section trace to a superseding or amending ADR. Sections marked **[SDS decision]** require ratification alongside this baseline.*
