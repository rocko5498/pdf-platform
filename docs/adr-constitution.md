# Engineering Constitution — Architecture Decision Records

**Project:** Open-source professional PDF platform (working name: *the project*)
**Document status:** Baseline set, ADR-001 … ADR-030
**Convention:** In the repository each ADR lives as `docs/adr/NNNN-title.md` (see ADR-024). ADRs are immutable once **Accepted**; changes require a superseding ADR that links back. Statuses: Proposed / Accepted / Superseded-by-NNNN / Deprecated.
**Provenance:** Every decision below codifies conclusions from the six-stage research program (capability map, format internals, competitor teardowns, open-source deep dives, user research, opportunity thesis). Research citations live in the research corpus, referenced here as [R1–R6].

---

## ADR-001 — Project Vision and Non-Negotiable Product Values

**Status:** Accepted

**Context.** User research [R5] shows the market's dominant pains are trust and engineering-discipline failures (subscription coercion — FTC/DOJ-documented; interface churn; bloat; silent file corruption), not missing features. Incumbents are structurally unable to fix these (97% subscription revenue; growth-driven UI orgs). Competitor analysis [R3] shows performance positioning decays without governance (Foxit) and that scope discipline is a durable identity (SumatraPDF).

**Problem statement.** A ten-year project needs a constitution-level statement of what it is, what it refuses to become, and which properties are enforced rather than aspired to — otherwise feature pressure and contributor drift reproduce the incumbents' failures.

**Decision.** The project is a fully open-source, native, cross-platform (Windows/macOS/Linux) PDF platform targeting professional Acrobat workflows, governed by five enforced values:
1. **Performance is contractual.** Published budgets (cold start, time-to-first-page, scroll frame time, memory per page, save latency) are CI release gates (ADR-023).
2. **Trust by construction.** No accounts, no telemetry by default, no network traffic without visible user cause, reproducible builds, local-first everything.
3. **Interface stability contract.** Acrobat-classic mental model and shortcuts; UI changes are versioned and opt-in; a "classic behavior" toggle is never removed (ADR-030).
4. **File fidelity above features.** The application never silently transcodes or degrades a document; incremental, surgical mutation is the only default write path (ADR-006, ADR-012).
5. **Honest capability boundaries.** Where we cannot act safely (e.g., editing text in an incomplete font subset), we say so explicitly rather than corrupt silently.

Non-goals for the foreseeable horizon: cloud service dependency, full XFA implementation, Office-suite replication, mobile clients.

**Alternatives considered.** (a) Feature-parity-first roadmap chasing Acrobat's checklist — rejected: replicates the incumbent's mass without its moat and ignores what users actually churn over. (b) Viewer-only scope (Sumatra model) — rejected: abandons the editing/signing/redaction wedges where open source has no champion. (c) Web/hybrid delivery — rejected in stack ADRs; conflicts with values 1 and 2.

**Trade-offs.** Enforced budgets slow feature velocity; the stability contract limits UX experimentation; refusing cloud features cedes some collaboration convenience in the near term.

**Consequences.** Every subsequent ADR must be traceable to these values. Marketing claims become falsifiable (budgets, reproducible builds, redaction verification). Contributors gain a rejection rationale that is impersonal and stable.

**Future considerations.** An optional, self-hostable collaboration server (opportunity O-collab) may extend value 2 rather than violate it; requires its own ADR when scheduled.

---

## ADR-002 — Rust for the Application Core

**Status:** Accepted

**Context.** PDF is a top-tier hostile-input format: decades of Acrobat CVEs (JBIG2, fonts, JavaScript), Ghostscript's interpreter-escape history, and PDFium's permanent fuzzing regime all demonstrate that the parsing/interpretation layer is the attack surface [R2, R3]. The core will hold the document object model, mutation engine, undo journal, search, signing, and plugin host — the code that must not have memory-corruption classes of bugs.

**Problem statement.** Choose the implementation language for all document-domain logic, optimizing for memory safety on untrusted input, performance parity with C++, long-term maintainability, and contributor recruitment over a decade.

**Decision.** All domain logic — everything that can parse, mutate, or interpret document data — is written in Rust. C++ exists only in the thin Qt shell (ADR-003) and inside vendored third-party engines behind the engine boundary (ADR-005).

**Alternatives considered.** (a) **C++** (the Okular/MuPDF path): maximum ecosystem maturity, but chooses an ongoing memory-safety CVE tax by construction for exactly the code where it costs most; modern C++ mitigations are convention, not guarantee. Rejected. (b) **Go:** memory-safe and productive, but GC pauses in the render path, weaker FFI ergonomics against C engines, and no story for the zero-copy/mmap patterns our memory philosophy requires (ADR-011). Rejected. (c) **Swift/C#/Java cores:** each ties the core to a runtime and platform gravity inconsistent with a neutral, embeddable headless library (opportunity O8). Rejected. (d) **Mixed C++/Rust core:** maximizes both toolchains' friction with neither's guarantees. Rejected.

**Trade-offs.** Smaller hiring/contributor pool than C++ for legacy PDF expertise; FFI boundaries to PDFium and Qt must be engineered deliberately (ADR-004, ADR-005); compile times tax CI (mitigated in ADR-029).

**Consequences.** `unsafe` becomes a governed resource (ADR-027). The core is publishable as crates, enabling the CLI/headless ecosystem position. Fuzzing yields logic bugs, not exploit primitives, in our own code.

**Future considerations.** If a pure-Rust engine (hayro lineage) matures to fidelity parity, the last large C/C++ mass inside the trust boundary disappears (ADR-005).

---

## ADR-003 — Qt Widgets as the Desktop Shell

**Status:** Accepted

**Context.** Requirements: Acrobat-class UI density (docking, toolbars, complex dialogs), best-available accessibility (screen readers are both an ethical requirement and a product wedge — PDF remediation users), native printing dialogs, IME correctness, and three first-class desktop platforms. Research verdicts [R-stack]: Rust-native toolkits (Iced, Slint, egui) lack mature accessibility, docking, and print infrastructure today; webview stacks (Electron, Tauri) fail performance/native values; Okular and Master PDF Editor are existence proofs for Qt in exactly this application class.

**Problem statement.** Select the UI technology delivering professional desktop chrome and accessibility now, without contradicting the ten-year Rust trajectory.

**Decision.** The shell is **Qt 6 Widgets** (not QML), written in C++, deliberately thin: windows, docking, menus, dialogs, print integration, accessibility surface (QAccessible), and a document canvas that composites tiles produced by the core. The shell holds no document state and contains no domain logic (enforced by ADR-026 and review policy).

**Alternatives considered.** (a) **QML/Qt Quick:** better for fluid custom UI, worse for dense classic desktop chrome, weaker widget-accessibility maturity, adds a JS layer to the trusted UI process. Rejected for the primary shell; permissible later for isolated views if an ADR justifies it. (b) **Slint:** workable licensing (GPL route) but thin desktop widget set and single-vendor governance; we would build toolkit infrastructure instead of PDF features. Rejected for now. (c) **Iced:** best-governed Rust option (COSMIC-proven) but accessibility/IME/docking gaps disqualify it for a remediation-grade product today. Rejected for now, retained as the designated future-shell candidate. (d) **GTK:** excellent Rust bindings, inadequate Windows/macOS citizenship. Rejected.

**Trade-offs.** Two languages in-repo (contributor friction, mitigated by making the shell boring and small — target ≤15% of code); LGPL compliance obligations (dynamic linking, standard and manageable); exposure to Qt Company stewardship drift (mitigated: Widgets is the most stable, least monetized Qt layer, and the KDE Free Qt Foundation agreement backstops it).

**Consequences.** Accessibility, printing, and platform integration are solved by adoption, not construction. The FFI boundary (ADR-004) becomes a permanent architectural feature. Shell replaceability is a designed property, verified by the headless CLI sharing the same core API.

**Future considerations.** Re-evaluate Rust-native shells circa 2029–2030 against a written checklist (AccessKit maturity, docking, print, IME, RTL). Migration would be a shell rewrite, not an application rewrite — that is the point of this architecture.

---

## ADR-004 — Rust ↔ Qt Communication Strategy

**Status:** Accepted

**Context.** The shell (C++/Qt, UI thread) and core (Rust, multi-threaded, fronting worker processes) must exchange commands, events, and rendered tiles at 60–120 Hz UI cadence without leaking domain types across the boundary or letting the boundary ossify into a second API surface.

**Problem statement.** Define the FFI mechanism, the protocol shape, and the threading/ownership rules at the language boundary such that the boundary stays small, auditable, and replaceable.

**Decision.** A **single, narrow, message-oriented boundary**:
1. Mechanism: the `cxx` crate generating a bidirectional bridge, wrapped in one dedicated pair (`ffi` crate ↔ shell `bridge/` module). No other translation unit in either language may declare cross-language items.
2. Shape: a **command/event protocol**, not an object model. The shell sends serialized commands (open, navigate, request-tiles, apply-annotation…); the core emits events (document-opened, tiles-ready, state-changed, error). Payload types are defined once, in Rust, in a protocol crate; large payloads (tiles) pass as shared-memory handles + descriptors, never copied through the bridge.
3. Threading contract: the bridge is the only place threads meet. Core→shell events are marshaled onto the Qt main thread via a queued dispatcher; the shell never blocks on the core (all request/response is asynchronous with correlation IDs); the core never calls Qt.
4. The same protocol crate drives the headless CLI, guaranteeing the shell has no privileged capabilities.

**Alternatives considered.** (a) **cxx-qt (KDAB):** attractive QObject integration, but it invites domain objects to grow Qt-shaped surfaces, thickening exactly the boundary we need thin; adopting it for the dispatcher glue only is permitted, for the protocol it is not. (b) **Raw C ABI (`extern "C"`) hand-rolled:** maximally portable, maximally error-prone; `cxx` gives the same control with checked signatures. Rejected. (c) **Full IPC between shell and core processes:** cleanest isolation, but the core's coordinator must live somewhere with UI-latency access; pushing even the coordinator out of process adds latency and serialization for no security gain (workers are already out-of-process, ADR-008). Rejected. (d) **Embedding a scripting/JSON pipe:** untyped boundaries rot. Rejected.

**Trade-offs.** Command/event indirection is more ceremony than direct calls; asynchronous-only interaction forces the shell to model pending states (which the UX needs anyway for honesty about long operations).

**Consequences.** The boundary is testable in isolation (protocol golden tests), the shell is mechanically replaceable, and a future Rust shell deletes the bridge without touching the protocol. Bridge code receives mandatory two-reviewer policy (ADR-027).

**Future considerations.** If plugin UIs (ADR-014) need shell embedding, they extend the event protocol rather than gaining their own FFI.

---

## ADR-005 — PDF Rendering Engine Strategy

**Status:** Accepted

**Context.** Rendering fidelity against decades of malformed real-world files is the single hardest asset to build [R2, R3]. Candidates: PDFium (BSD-3, battle-hardened, weak editing API, Chrome-driven roadmap), MuPDF (best architecture, AGPL — license-incompatible with our linking plans, usable as oracle), Poppler (GPL, viewer-grade), hayro (pure Rust, corpus-driven, pre-optimization maturity). No engine satisfies rendering + editing + license simultaneously.

**Problem statement.** Obtain best-available fidelity and security now, full structural editing capability that no external engine provides, and freedom to change engines over a decade — without coupling the application to any engine's API or roadmap.

**Decision.** A three-part strategy:
1. **Capability-trait engine boundary.** The core defines Rust traits per capability — rasterize page region, extract text-with-geometry, enumerate structure, forms surface, low-level object access — informed by Okular's Generator interface and MuPDF's device pipeline. All application code targets traits only.
2. **PDFium is the launch rasterizer/extractor** (vendored, sandbox-hosted per ADR-008, Skia path where stable, V8/XFA compile flags governed by ADR-017 and ADR-001 non-goals), integrated via maintained bindings audited against the trait contract.
3. **Structural ownership is ours.** Parsing, the COS object store, mutation, incremental save, signing, and redaction are implemented in our Rust document model (ADR-006), with qpdf as the correctness reference. PDFium renders; it does not own the file.
Additionally: hayro is integrated behind the same traits as an experimental backend from early on, giving us differential rendering data and a migration path; MuPDF (via its tools, unlinked) and Acrobat serve as external oracles in the test lab (ADR-022).

**Alternatives considered.** (a) MuPDF + AGPL application: viable license posture but poisons the permissive plugin/SDK ecosystem and enterprise adoption path chosen in governance; rejected. (b) PDFium for everything including mutation: its editing API cannot express incremental update, signatures, or redaction correctly; rejected. (c) Build our own renderer first: multi-year fidelity climb before first release; rejected. (d) Poppler: fidelity and editing depth below requirements; rejected.

**Trade-offs.** Two object views of one file (our COS store + PDFium's parse) cost memory and demand consistency discipline (mutations flow: our store → serialized bytes → engine reload of changed pages); vendoring PDFium imports Chromium build complexity (contained in CI, prebuilt artifacts for contributors, ADR-029).

**Consequences.** Engine swap is a backend implementation, not a rewrite. Our security posture inherits Google's fuzzing regime for rendering while our own mutation code is memory-safe. The trait layer becomes a public seam for the headless SDK.

**Future considerations.** Promotion criteria for hayro (or successor) to default rasterizer: ≥ PDFium pass rate on the corpus, performance within budgets, one year as opt-in default in nightly.

---

## ADR-006 — Document Object Model

**Status:** Accepted

**Context.** Format research [R2]: a PDF is a random-access object database with an append-only update mechanism; competitor research [R3]: every architecture that transcodes documents into a foreign model (LibreOffice Draw, Ghostscript pdfwrite) destroys fidelity, signatures, and structure. User pain #5 is precisely this silent destruction.

**Problem statement.** Define the in-memory representation of an open document supporting lazy loading of gigabyte files, surgical mutation, signature-preserving saves, unlimited undo, and multi-reader/single-writer concurrency.

**Decision.** A **two-layer, store-backed model**:
1. **COS layer:** an indexed object store over the memory-mapped file — xref-driven, lazily materializing objects on demand, with a **copy-on-write overlay**: original bytes are immutable ground truth; every mutation creates a new object version in the overlay keyed by revision. Repair/reconstruction (qpdf-style xref rebuild, leniency ledger recording every deviation tolerated) lives here.
2. **Semantic layer:** typed façades (Page tree, Annotations, AcroForm fields, Outline, Signatures, StructTree) that interpret COS objects on access and write through to the overlay. Façades are views, not copies — there is deliberately no third "app model" to drift out of sync.
Invariants: unparsed/unknown objects are preserved byte-exact; no operation may rewrite objects it did not logically touch; every mutation is expressed as a Command (ADR-013) producing a delta over the overlay.

**Alternatives considered.** (a) Full eager parse into an owned tree: simple, but breaks lazy loading, explodes memory on large files, and invites transcoding-style saves. Rejected. (b) Direct mutation of a single mutable store without CoW: cheaper, but forfeits free undo/journal/crash-replay and complicates concurrent read-while-render. Rejected. (c) Delegating the model to PDFium's objects: under-expressive and roadmap-captive (ADR-005). Rejected.

**Trade-offs.** CoW overlays cost bookkeeping and force a serialization step before the render engine sees edits; façade-on-demand costs repeated interpretation (mitigated by memoization with revision-keyed invalidation).

**Consequences.** Incremental save (ADR-012) is a serialization of the overlay — the model and the format share one theory of change. Undo, crash recovery, and the revision-history feature (opportunity O6) all derive from the same primitive. "What changed" is always answerable.

**Future considerations.** A content-stream-level micro-model (operators, text runs) will be layered for editing phases; it must obey the same CoW/command discipline.

---

## ADR-007 — Rendering Pipeline

**Status:** Accepted

**Context.** Budgets (ADR-001/023): sub-second open, first page < 300 ms typical, smooth scroll on 2,000-page documents, bounded memory. Prior art: Okular's tiled priority rendering, pdf.js's worker split, MuPDF's display lists.

**Problem statement.** Define how pixels get from hostile file to screen within budgets, across process boundaries, at arbitrary zoom, with cancellation and caching.

**Decision.** A **tile-based, pull-driven, multi-process pipeline**:
1. The shell's canvas publishes a viewport (pages × rects × scale × rotation); the core's render scheduler decomposes it into fixed-size device-space tiles.
2. Tiles are rasterized in sandboxed document workers (ADR-008) via the engine trait, written into shared-memory buffers, and announced by handle over the event protocol; the shell uploads them as GPU textures and composites (zoom/pan animate on the GPU against stale tiles while fresh ones stream in).
3. Scheduling: priority = visibility > prefetch ring > thumbnails > background (index/thumbnail jobs, ADR-009); every request carries a generation counter — viewport changes cancel stale work cheaply.
4. Cache: a byte-weighted LRU of tiles keyed (revision, page, scale-bucket, tile-coord) with budgets from ADR-011; invalidation is revision-keyed, so an edit invalidates only touched pages' tiles.
5. Progressive strategy: low-scale whole-page raster first paint, tiles refine; text/selection overlays render in the shell from extracted geometry, never baked into tiles.

**Alternatives considered.** (a) Whole-page rasters only: memory explodes at high zoom (Okular's documented scar). Rejected. (b) Vector display-list replay in the shell: elegant, but requires trusting/porting interpretation into the UI process — violates the security model. Deferred as an *optional* worker-side optimization (record once, re-raster at scales) behind the engine trait. (c) Synchronous render-on-paint: unacceptable jank on hostile files. Rejected. (d) GPU rasterization end-to-end (Vello-class): promising, immature for full PDF semantics; revisit with hayro's GPU work (ADR-005 futures).

**Trade-offs.** Shared-memory tile transport is platform-specific plumbing ×3 OSes; progressive refinement briefly shows lower-fidelity content (accepted — matches user expectation and honesty value).

**Consequences.** Scroll performance is decoupled from document hostility; a crashed worker costs cached tiles, not the app; benchmarks can target each stage independently.

**Future considerations.** Color-managed and overprint-simulating tile variants (prepress phase) become additional cache dimensions; plan key-space now.

---

## ADR-008 — Multi-Process Architecture

**Status:** Accepted

**Context.** Ghostscript's escape history, Acrobat's retrofitted Protected Mode, Chrome's sandboxed PDFium, and pdf.js's worker split independently converge on the same lesson [R3, R4]: document interpretation must not share a failure or security domain with the application.

**Problem statement.** Choose the process topology balancing isolation, memory, IPC complexity, and crash-blast-radius for a multi-document desktop editor.

**Decision.** Three process classes:
1. **UI/coordinator process:** Qt shell + Rust core coordinator (document model overlays, schedulers, undo journals, plugin host control). No document *interpretation* executes here.
2. **Document worker (one per open document):** hosts the engine backend (PDFium), content-stream interpretation, text extraction, and any PDF-JavaScript (ADR-017), fully sandboxed — seccomp-bpf + namespaces (Linux), AppContainer + job objects (Windows), Sandbox/`sandbox_init` profiles (macOS); no network, filesystem restricted to brokered handles.
3. **Utility worker pool (shared, low-priority):** OCR, indexing, optimization, thumbnail farms (ADR-009), same sandbox profile.
Crash policy: worker death is an event; the coordinator's model + journal permit transparent worker respawn and state replay (ADR-021).

**Alternatives considered.** (a) Single process, threads only: simplest, and one malformed file kills every open document — indefensible given the threat model. Rejected. (b) One worker per *page/task*: Chrome-grade isolation, desktop-inappropriate memory and spawn costs. Rejected. (c) Worker per document *group* (shared): saves memory, couples unrelated documents' fates; rejected as default, allowed as a low-memory-mode degradation policy. (d) Sandboxing only "risky" operations: the parse itself is the risk; there is no safe subset. Rejected.

**Trade-offs.** Per-document worker overhead (~tens of MB) and triple-platform sandbox engineering, which is genuinely expensive and specialized; IPC/shared-memory machinery must exist before feature work — a deliberate front-loaded cost.

**Consequences.** The reliability value becomes architecture, not aspiration: hostile file ⇒ one tab's worker restarts. Security audits scope to broker interfaces. The coordinator/worker split forces the clean model/engine separation ADR-005/006 already require.

**Future considerations.** Plugin execution placement (ADR-014/015) reuses the utility-pool sandbox; WASM adds a second, inner isolation layer.

---

## ADR-009 — Background Work System

**Status:** Accepted

**Context.** Long operations — OCR, cross-document indexing, optimization, batch pipelines, thumbnail generation, PDF/A validation — must never contend with interactive latency budgets, must be cancellable, resumable where possible, and honest in the UI (Stirling-PDF demand signal for pipeline UX [R4, R5]).

**Problem statement.** Define a uniform job model so every long operation gains queuing, priority, progress, cancellation, persistence, and CLI parity without bespoke plumbing.

**Decision.** A core-owned **job system**: jobs are declarative descriptions (operation + inputs + parameters) executed in the utility worker pool (ADR-008) under a scheduler with priority classes (interactive-adjacent > user-initiated batch > opportunistic maintenance), OS-level low priority for the pool, structured progress events over the standard protocol, cooperative cancellation tokens, and a persisted queue so batch pipelines survive restarts (ADR-021). Jobs compose into pipelines (DAGs) — the primitive behind both the batch UI and the CLI (`--pipeline` files), guaranteeing GUI/CLI feature parity by construction.

**Alternatives considered.** (a) Ad-hoc threads per feature: the incumbent pattern; unauditable, un-cancellable, duplicated progress UX. Rejected. (b) Embedding a general async runtime in the coordinator for background work: conflates I/O concurrency with compute parallelism and infects the core with runtime coloring (see ADR-010). Rejected. (c) External queue daemon: server-shaped over-engineering for a desktop app. Rejected.

**Trade-offs.** Declarative jobs are more upfront design than fire-and-forget threads; persistence adds a small state store to maintain.

**Consequences.** Every heavy feature ships with progress/cancel/queue for free; the opportunity O8 CLI is the same engine; benchmarking can replay recorded pipelines.

**Future considerations.** Plugin-defined job types (ADR-014) join the same scheduler with capability-scoped quotas.

---

## ADR-010 — Threading Model

**Status:** Accepted

**Context.** Three concurrency regimes coexist: Qt's single UI thread; the coordinator's event-driven state; workers' data-parallel rasterization/extraction. Mixed paradigms (locks + async + signals) are where large desktop codebases rot.

**Problem statement.** Prescribe one concurrency discipline per regime and forbid the rest.

**Decision.**
1. **Shell:** Qt main thread only; zero worker threads in C++ (anything expensive is a core request). Event dispatch from the bridge is queued-connection marshaled.
2. **Coordinator core:** message-driven ownership — each document's coordinator state is owned by a single logical task (actor-style: owned state + channel inbox); cross-document services (cache manager, job scheduler, plugin host) likewise. **No async runtime in the document core**: plain threads + channels; I/O concurrency needs are modest and explicit. Shared-state locking is the exception and requires justification in review (allowed: the tile cache's sharded index).
3. **Workers:** data-parallel compute via a work-stealing pool (rayon-class) inside each worker; blocking is fine — workers are compute processes by definition.
4. Global rules: no lock is held across a channel send or FFI call; every thread/task has a name and appears in diagnostics (ADR-020); the protocol layer is the only cross-regime membrane.

**Alternatives considered.** (a) Tokio-style async throughout the core: excellent for network servers, wrong shape for CPU-bound document work; function coloring would infect the public core API used by embedders. Rejected. (b) Free-threaded shared-state core with fine-grained locking: the classic path to deadlock archaeology; Rust helps but does not prevent lock-order bugs. Rejected. (c) Fully single-threaded core: simple, cannot exploit multi-page parallelism. Rejected.

**Trade-offs.** Actor-owned state serializes some per-document operations (acceptable: a document has one user); channel plumbing is verbose relative to method calls.

**Consequences.** Concurrency bugs localize to ownership boundaries; the model is teachable to contributors in one document; deterministic replay of coordinator inboxes becomes a test technique (ADR-022).

**Future considerations.** If a plugin or collaboration server later needs async I/O, it lives in its own crate at the edge, adapting to channels at the boundary.

---

## ADR-011 — Memory Management Philosophy

**Status:** Accepted

**Context.** Pain #3/#6 [R5]: incumbents collapse on large documents. Budgets (ADR-001) demand predictable memory on gigabyte, 2,000-page files. The format supports laziness natively (ADR-006); the pipeline caches aggressively (ADR-007).

**Problem statement.** Establish rules that make memory a budgeted, observable resource rather than an emergent property.

**Decision.**
1. **Files are memory-mapped, never slurped.** The COS layer materializes objects on demand; decoded stream data is cache, not state.
2. **All caches are weighted, bounded, and centrally accounted.** A global memory governor assigns budgets (tile cache, decoded-object cache, glyph/font cache, text-geometry cache) scaled to system RAM, with pressure-driven eviction (weight-based LRU; revision-keyed invalidation) and an OS-memory-pressure hook per platform.
3. **Workers use arena/region allocation for per-render scratch** — a page render allocates into an arena dropped wholesale, bounding fragmentation and making per-render cost measurable.
4. **No unbounded collection may cross a release gate**: any container that grows with document size or session length must document its bound or eviction policy (clippy-lint + review checklist, ADR-027).
5. Degradation ladder: under pressure, drop prefetch ring → downscale cached tiles → shrink decoded caches → (last) collapse to low-memory shared-worker mode (ADR-008 alt-c) — never crash, never block the UI thread on reclamation.

**Alternatives considered.** (a) "Rust means we don't leak" laissez-faire: confuses safety with economy; caches leak by policy, not by bug. Rejected. (b) Per-feature ad-hoc caches: the incumbent pattern behind pain #3. Rejected. (c) Custom global allocator tuning first: premature; measure with the governor before allocator work. Deferred.

**Trade-offs.** Central accounting adds indirection to every cache insert; arenas complicate code that wants to retain render byproducts (they must explicitly promote data out).

**Consequences.** "Memory per open page" becomes a benchmarked, regression-gated number (ADR-023). Large-document behavior is a designed curve, not folklore. Diagnostics (ADR-020) can display the governor's ledger live.

**Future considerations.** io_uring/overlapped-IO readahead tuning per platform once profiles justify it.

---

## ADR-012 — Incremental Save Strategy

**Status:** Accepted

**Context.** Format research [R2]: incremental update is PDF's native change mechanism; it preserves signatures, enables instant saves, and retains history. Competitor research [R3]: transcoding saves (Draw, pdfwrite) are the ecosystem's chief fidelity destroyer. Signatures (opportunity O4) and history (O6) both depend on getting this exactly right.

**Problem statement.** Define the write path: when we append, when we rewrite, how saves are atomic, and how history/privacy interact.

**Decision.**
1. **Default save = incremental append**, always: serialize the CoW overlay (ADR-006) as new object versions + xref section + trailer with `/Prev`. Untouched bytes are never rewritten. Post-signature edits are appended in DocMDP-legal form or refused with explanation (honesty value).
2. **Atomicity:** write-to-temp + fsync + atomic rename where the filesystem allows; where rename-over is unsafe (locked files, some network shares), fall back to append-in-place guarded by a journal record (ADR-021) enabling truncation-rollback of a torn append.
3. **Full rewrite is a distinct, explicit operation** ("Optimize/Save As Clean"): linearization, object-stream repacking, garbage collection, optional flatten-history/sanitize — clearly labeled as signature-breaking and history-destroying, with a pre-flight report of what will be lost.
4. Every save records a revision entry (id, timestamp, operation summary) powering the history timeline (O6).

**Alternatives considered.** (a) Full rewrite on every save (most libraries' default): simpler writer, destroys signatures/history, O(file) latency — violates three project values. Rejected. (b) Always append, never offer rewrite: files grow monotonically and privacy-sensitive history accumulates invisibly. Rejected — hence the explicit dual-mode. (c) Sidecar change-files (Xournal++ overlay model): breaks interop with every other reader. Rejected.

**Trade-offs.** Incrementally-saved files are larger until optimized; append-fallback atomicity is more intricate than rename-only; the writer must implement both classic xref tables and xref/object streams to match the file's existing style.

**Consequences.** Save latency is O(change), satisfying its budget trivially. The save path, undo journal, and history feature share one serialization theory. Sanitize/flatten becomes a conscious, auditable user act — closing the incremental-update privacy footgun.

**Future considerations.** Background opportunistic optimization (utility pool) may offer "compact now?" hints when append overhead crosses a threshold — suggestion only, never automatic.

---

## ADR-013 — Undo/Redo Architecture

**Status:** Accepted

**Context.** No PDF editor ships trustworthy deep undo; users fear silent damage (pain #5). Our model (ADR-006) already expresses every mutation as a delta; the format persists revisions (ADR-012). Prior art: Blender/Krita command stacks [R4].

**Problem statement.** Provide unlimited, grouped, persistent, crash-surviving undo across all mutation features, uniformly — including future plugin-initiated edits.

**Decision.** **Command-journal architecture:**
1. Every mutation is a **Command**: named, parameterized, producing (and owning) its forward delta over the CoW overlay and sufficient state for inversion. Commands compose into user-visible groups (one "Redact page 3" = many object deltas).
2. The **journal** is the append-only log of command groups per document, held by the coordinator, **persisted to a sidecar autosave journal** between saves (ADR-021) and reconciled with file revisions at save time — undo therefore survives crashes and, across sessions, aligns with the revision timeline (O6): undo within unsaved work is command-inversion; stepping behind a saved revision is presented as history rollback, a distinct, explicit act.
3. Inversion strategy: prefer stored inverse deltas (cheap, exact under CoW); memento snapshots only for operations whose inverse is impractical (declared per command type, reviewed).
4. Plugins and JS (ADR-014/017) can only mutate via Commands — there is no side door, so their edits are undoable and attributable by construction.

**Alternatives considered.** (a) Whole-model snapshots per step: simple, memory-hostile on large documents. Rejected except as declared mementos. (b) UI-level undo (Qt's QUndoStack as source of truth): puts document truth in the shell — violates ADR-003/004. Rejected (the shell may mirror the stack for menus only). (c) CRDT/OT event models: solves multi-writer collaboration we don't have; cost without requirement. Rejected for now.

**Trade-offs.** Command discipline taxes every feature with design work up front; journal persistence adds I/O on mutation (batched, budgeted).

**Consequences.** "Undo anything, always, even after a crash" becomes a headline property derived from architecture. Command names give free audit trails and macro/batch recording later.

**Future considerations.** Command log replay is the natural seam for a future collaboration layer, should ADR-001's non-goal ever be revisited by a superseding ADR.

---

## ADR-014 — Plugin Architecture

**Status:** Accepted

**Context.** Acrobat's native-DLL SDK created its ecosystem lock-in and its crash/security coupling [R3]. Zed/Lapce demonstrate sandboxed WASM extension ecosystems [R4]. Our values require plugins that cannot crash the host, exfiltrate documents, or freeze the UI.

**Problem statement.** Define the extension model: execution substrate, capability surface, API stability, and distribution posture, for a decade of third-party code.

**Decision.**
1. **Plugins are sandboxed WASM components** (runtime per ADR-015) executing in utility-pool worker processes (ADR-008) — double isolation (WASM sandbox inside OS sandbox). No native-code plugin path exists.
2. **Capability-scoped API:** a plugin's manifest declares needs (read document text, add annotations, register job type, add tool panel, network access); users grant at install with per-capability visibility; undeclared capabilities are unlinkable, not merely denied.
3. Mutation exclusively through Commands (ADR-013); document access through the same semantic-layer API the application uses — plugins are clients of the public core surface, keeping us honest about API quality.
4. UI extension is declarative: plugins contribute panels/menu items/tool descriptors rendered by the shell from schema — plugin code never runs in the UI process.
5. The plugin API is **versioned independently and semver-guaranteed** (ADR-030); a compatibility test-kit ships with the SDK.

**Alternatives considered.** (a) Native dynamic libraries (Acrobat model): maximum power, in-process crash/exploit coupling, ABI hell across 3 OSes × versions. Rejected outright. (b) Embedded scripting language (Lua/JS/Python): friendlier authoring but single-language, and engine embedding still demands the same sandboxing work with weaker guarantees. Rejected as the *architecture* (a Lua/JS authoring SDK compiling to WASM remains possible). (c) Out-of-process native plugins over IPC: safe but heavyweight per plugin and OS-specific packaging pain for authors. Rejected.

**Trade-offs.** WASM constrains plugin access to native OS facilities (deliberate); performance-hungry plugins pay the WASM tax (acceptable: heavy lifting belongs in core feature requests); ecosystem bootstrap is slower than "just load a DLL."

**Consequences.** A malicious or broken plugin is a killed worker and a revoked capability, not a CVE. The public core API gains a second demanding client (first: the CLI), forcing genuine quality.

**Future considerations.** A curated registry with signed packages and static capability audit is a later governance ADR; design manifests now so signing slots in cleanly.

---

## ADR-015 — WASM Plugin Runtime

**Status:** Accepted

**Context.** ADR-014 fixes WASM; the runtime, interface-definition, and API-shape choices remain. Candidates: Wasmtime (Bytecode Alliance, reference Component Model implementation), Wasmer, WasmEdge, browser-engine embedding.

**Problem statement.** Select the runtime and interface technology giving typed, versionable, multi-language plugin interfaces with credible ten-year stewardship.

**Decision.** **Wasmtime + the WebAssembly Component Model, with WIT-defined interfaces.**
1. All plugin APIs are specified in WIT; the SDK generates bindings for Rust, JS/TS (via componentize toolchains), Python, and Go as toolchains mature — language-plural authoring against one typed contract.
2. WASI is exposed only through our capability broker (ADR-014); raw WASI filesystem/network is never granted directly.
3. Execution: instances are pooled in utility workers; fuel/epoch interruption enforces CPU quotas; store-per-instance memory limits enforce RAM quotas — plugins are preemptable and budgeted like jobs (ADR-009).
4. Versioning: WIT worlds are the semver unit; deprecated interfaces ship alongside successors for ≥2 release trains (ADR-030).

**Alternatives considered.** (a) **Wasmer:** capable runtime, but the Component Model/WIT center of gravity, standards work, and security review mass sit with Wasmtime/Bytecode Alliance; betting the API layer elsewhere risks dialect drift. Rejected. (b) **WasmEdge:** strong in server/edge niches, weaker desktop embedding story. Rejected. (c) **Core-WASM-only with hand-rolled C-ABI marshaling (Zed's early approach):** proven, but re-invents interface typing we'd maintain forever; the Component Model is precisely this problem solved by a standards body. Rejected. (d) Embedding V8/QuickJS for JS plugins natively: single-language and duplicates sandbox engineering. Rejected (JS authors are served via componentization).

**Trade-offs.** Component Model toolchains are young — some guest languages lag; binding generation adds build machinery to the SDK; fuel metering costs a few percent execution overhead.

**Consequences.** Plugin interfaces are typed, diffable, and mechanically checkable for breakage in CI; quotas make "a plugin froze the app" structurally impossible; the same WIT surface can later expose the headless core to embedders.

**Future considerations.** If Component Model tooling stalls (review annually), the WIT contracts still stand — they'd retarget to generated C-ABI shims without changing plugin-visible semantics.

---

## ADR-016 — Security Model

**Status:** Accepted

**Context.** PDF is a hostile-input format with an exploit industry [R2, R3]; our differentiation includes trust (ADR-001). Assets: user documents (confidentiality/integrity), the host system, signature trust decisions. Adversaries: malicious documents, malicious plugins, malicious links/actions, and (for signatures) forgery attempts.

**Problem statement.** Define the threat model, isolation boundaries, brokered privileges, and security process as constitution, before code exists.

**Decision.**
1. **Trust zones:** (Z0) UI/coordinator — trusted, minimal parsing; (Z1) document workers — untrusted computation, fully sandboxed, no network, brokered file handles only; (Z2) plugin instances — untrusted, WASM-inside-Z1-grade sandbox, capability-brokered; (Z3) the document itself — always adversarial data.
2. **Broker principle:** all privileges (file open/save, printing, clipboard, any network) execute in Z0 via audited broker interfaces with per-call validation; Z1/Z2 request, never possess.
3. **Interactive-content policy:** URI actions require visible consent with full URL display; `/Launch` and embedded-executable actions are refused unconditionally; JavaScript per ADR-017; remote resource loading (e.g., referenced content) is off by default, per-document opt-in.
4. **Memory-safety policy:** all Z0 parsing in safe Rust; `unsafe` per ADR-027; C/C++ mass (PDFium, Qt) confined to Z1 or the UI framework respectively.
5. **Process:** continuous fuzzing of every input surface (parsers, filters, fonts, protocol) with OSS-Fuzz enrollment at public launch; a published security policy, private disclosure channel, and CVE handling SLA; sandbox escapes are release-blocking regardless of schedule; an annual external audit of broker + bridge is budgeted.
6. **No security theater:** permission flags are honored by default but documented as advisory; encryption UX states exactly what is and isn't protected.

**Alternatives considered.** (a) Single-process with hardened parsing only: refuted by the entire industry's history (Acrobat pre-sandbox, Ghostscript). Rejected. (b) Sandboxing later, features first: retrofit cost is maximal (Adobe's own path); isolation is cheapest at foundation. Rejected. (c) VM-grade isolation (per-document microVMs): security surplus, desktop-hostile resource cost. Rejected.

**Trade-offs.** Brokered design taxes every privileged feature with interface design and review; triple-platform sandbox maintenance is permanent skilled work; refusing `/Launch` breaks a tiny population of legacy workflows (accepted, documented).

**Consequences.** Security claims become architectural and auditable — a marketable, falsifiable property (reproducible builds, ADR-029, complete it). The broker catalog doubles as the privilege documentation.

**Future considerations.** Site-isolation-style splitting of signature validation into its own worker once PAdES work lands, so trust decisions compute in a minimal zone.

---

## ADR-017 — PDF JavaScript Policy

**Status:** Accepted

**Context.** Embedded Acrobat-JS drives form validation/calculation in a meaningful fraction of enterprise forms [R1]; ignoring it causes *silent* wrong numbers — worse than failure. Full Acrobat-JS surface is enormous and historically exploit-rich. pdf.js ships a sandboxed subset; PDFium offers optional V8.

**Problem statement.** Decide whether, which subset, where, and under what user-visible policy document JavaScript executes.

**Decision.**
1. **Scoped support, phased:** implement the **forms subset** — field get/set, calculation/validation/format events, `AFNumber_*`/`AF*` utility family, calculation order — because that is where silent-wrongness lives. Document/app-automation surfaces (file I/O, network, UI scripting, annotations-by-script) are permanently out of scope absent a superseding ADR.
2. **Execution location:** inside the document worker (Z1) only, via the engine's V8 build initially, behind an `EngineScripting` trait so a leaner interpreter (QuickJS-class) can replace V8 if audit/footprint favor it; zero broker capabilities are reachable from script.
3. **Policy UX:** per-document indicator when scripts exist; default = enabled for the forms subset (silent-wrongness argument), with a global and per-document kill switch and an enterprise policy control (ADR-030 packaging); anything outside the subset no-ops and is *logged visibly* in diagnostics — honesty over emulation.
4. Every script-initiated field change flows through Commands (ADR-013): undoable, attributable.

**Alternatives considered.** (a) No JS at all: safest, silently mis-computes real forms — fails users invisibly. Rejected. (b) Full Acrobat-JS pursuit: years of surface, permanent attack area, chases a proprietary moving target. Rejected. (c) JS in the coordinator for latency: violates the security model outright. Rejected. (d) Default-off subset: honest but breaks the "form just works" moment that decides adoption; mitigated instead by strict subset + sandbox. Rejected.

**Trade-offs.** V8 inside workers costs binary size and memory per document (flag-gated builds; measure against a QuickJS spike); subset boundaries will generate "but Acrobat runs this" reports requiring a public compatibility table.

**Consequences.** Forms compute correctly for the common enterprise cases without inheriting Acrobat's automation attack surface; the compatibility table becomes a living spec of our subset.

**Future considerations.** XFA remains out of scope (ADR-001); if the subset table shows concentrated demand for a specific additional API, extend by superseding ADR with threat review.

---

## ADR-018 — OCR Strategy

**Status:** Accepted

**Context.** OCR demand is high (pain #9); recognition research moves fast (Tesseract → transformer engines); OCRmyPDF demonstrates the real engineering is pipeline + invisible-text-layer registration, not recognition [R4].

**Problem statement.** Deliver searchable-scans functionality that improves as OCR research evolves, without coupling the product to any engine.

**Decision.**
1. **Engine-plural boundary:** an `OcrEngine` trait consuming page rasters + hints, producing a normalized intermediate (text, boxes, confidence, orientation — hOCR/ALTO-equivalent). Recognition engines are pluggable backends and legitimate WASM-plugin territory (ADR-014).
2. **Default backend: Tesseract** (vendored, running in utility workers under full sandbox) for license, language breadth, and packaging maturity; a modern ML backend is evaluated per release against a fixed scan corpus and promoted when it wins on accuracy within budgets.
3. **The pipeline is ours and is the product:** preprocessing (deskew, despeckle, DPI normalization), correct **text-render-mode-3 layer generation registered under original images** via our writer, per-page skip logic for existing text layers (fixing Acrobat's notorious "renderable text" refusal — a named target), PDF/A-conform output option, all as jobs (ADR-009) hence batch- and CLI-capable.
4. JBIG2 policy: symbol-mode compression of OCR output is off by default with an explicit warning when enabled (Xerox substitution hazard) — fidelity value over bytes.

**Alternatives considered.** (a) Build recognition in-house: research treadmill outside our mission. Rejected. (b) Shell out to OCRmyPDF wholesale: proven pipeline but drags a Python runtime + Ghostscript (AGPL) into the product and forfeits integration (progress, undo, selective regions). Rejected as dependency; embraced as design reference and test oracle. (c) Cloud OCR default: violates trust values. Rejected (may exist later as an explicit opt-in plugin, never core).

**Trade-offs.** Tesseract's accuracy lags SOTA on hard scans until a promoted ML backend lands; owning preprocessing means owning its bugs.

**Consequences.** OCR quality can improve by backend swap without product change; "make searchable" works identically in GUI, batch, and CLI; the scan corpus becomes a permanent benchmark asset.

**Future considerations.** Layout-analysis outputs from ML backends feed the auto-tagging/remediation pipeline (opportunity O7) through the same intermediate representation — design it with structure fields now.

---

## ADR-019 — Search and Indexing Strategy

**Status:** Accepted

**Context.** In-document search is an MVP budgeted feature; cross-document indexing is demanded but must respect trust values (no cloud, no surprise disk usage). Extraction quality — ToUnicode pathology, ligatures, hyphenation, RTL/CJK — dominates search quality [R2].

**Problem statement.** Architect search so in-document find is instant, cross-document search is optional and local, and both share one extraction truth.

**Decision.**
1. **One extraction service:** the document worker produces a canonical per-page text model (Unicode runs + geometry + reading order when tagged), cached revision-keyed (ADR-011); selection, find, accessibility export, compare, and indexing all consume this single model — no feature re-extracts.
2. **In-document find:** streaming search over the text model with normalization (case, diacritics, ligature folding, soft-hyphen elision), operating page-window-first for instant first-hit, then completing in background; hit geometry drives shell overlay highlights (ADR-007).
3. **Cross-document index: opt-in, local, visible.** Tantivy-based index over user-designated folders, built by utility jobs (ADR-009), size-budgeted and inspectable/deletable in settings; watches for file changes; never indexes without explicit enrollment.
4. Extraction pathology handling is honest: pages whose text layer is unreliable (missing/lying ToUnicode) are flagged in diagnostics and offered OCR-assisted extraction (ADR-018) rather than silently searched wrong.

**Alternatives considered.** (a) Engine-native search calls per feature: duplicates pathology handling and diverges results across features. Rejected. (b) SQLite FTS instead of Tantivy: adequate for small corpora, weaker ranking/scaling and another C dependency in Z0; Tantivy is Rust-native and proven. Rejected. (c) Always-on background indexing of everything opened: better recall, violates the no-surprise-resource-use value. Rejected.

**Trade-offs.** The canonical text model costs memory (governed) and up-front design breadth (RTL, vertical text) before "simple find" ships; opt-in indexing yields worse out-of-box cross-document recall than incumbents' catalogs (accepted, honest).

**Consequences.** Search quality bugs are extraction bugs — one place to fix; compare-documents (later) inherits a mature text model for free; index privacy posture is defensible in one sentence.

**Future considerations.** Fuzzy/semantic search layers (user demand #11) attach above Tantivy without touching extraction; any embedding-based feature must re-pass the trust test (local models only).

---

## ADR-020 — Logging, Diagnostics, and Observability

**Status:** Accepted

**Context.** A multi-process system fails in cross-process narratives; contributors triage without vendor telemetry (trust values forbid default call-home); the leniency ledger (ADR-006) and JS no-op log (ADR-017) already promise user-visible diagnostics. qpdf's `--json` demonstrated inspectability's value [R4].

**Problem statement.** Give developers, contributors, and power users deep visibility while making privacy violations structurally difficult.

**Decision.**
1. **Structured tracing everywhere:** the Rust `tracing` ecosystem across coordinator and workers with cross-process span propagation over the protocol (every command/event/job carries a trace ID); the Qt shell logs into the same pipeline via the bridge. Ring buffers in memory by default; disk logging is explicit.
2. **Privacy by type system:** document content, file paths, and user strings are wrapper types that redact in `Display`/serialization; logging them raw requires an explicit `unredacted` call that only compiles in debug builds. No release log may contain document content — enforced by construction, not policy.
3. **User-facing diagnostics panel:** per-document report — leniency ledger (what we repaired/tolerated), unsupported-feature log (JS no-ops, unrendered constructs), memory governor ledger, worker restarts — turning our honesty value into UI.
4. **Inspector mode:** a qpdf-style object-model browser (COS tree, revisions, xref history) shipped in the product (behind an advanced flag) and the CLI — our own bug-report generator and the power-user wedge.
5. **Zero default telemetry.** Crash minidumps and diagnostics export are user-initiated artifacts the user can read before sending (ADR-021).

**Alternatives considered.** (a) printf/qDebug ad-hoc: incoherent across processes; rejected. (b) Opt-out telemetry "to improve the product": violates ADR-001 value 2 regardless of intent; rejected permanently. (c) Full OpenTelemetry stack: server-shaped weight; we take its concepts (spans, structured fields) via `tracing` without the collector apparatus. Rejected as dependency.

**Trade-offs.** Redaction-by-type adds friction to quick debugging (deliberate); we forfeit fleet-wide failure statistics and must compensate with excellent user-initiated reports and corpus growth.

**Consequences.** A bug report can contain a coherent multi-process trace with zero document content; the diagnostics panel differentiates the product; review can reject any raw-string logging mechanically.

**Future considerations.** An opt-in, fully local "flight recorder" (rolling trace on disk) for reproducing rare crashes; its own privacy review before shipping.

---


---

## ADR-021 — Crash Recovery and Data Durability

**Status:** Accepted

**Context.** Reliability is a constitutional value; multi-process design (ADR-008) makes worker death survivable by architecture, but coordinator death, power loss, and torn saves still threaten user work. The command journal (ADR-013) and incremental save model (ADR-012) provide the primitives.

**Problem statement.** Guarantee that no committed user action is lost to any single failure — worker crash, app crash, or interrupted save — and that recovery is automatic and comprehensible.

**Decision.**
1. **Worker crash:** coordinator detects death, respawns, replays document open + overlay state; in-flight renders reissue; the user sees a brief per-document notice and a diagnostics entry — never a dialog demanding action.
2. **Coordinator/app crash:** the persisted **sidecar journal** (ADR-013 — command groups since last save, written with bounded batching latency ≤ a stated budget, e.g., committed within 2 s or N commands of any mutation) enables next-launch recovery: original file + journal replay reconstructs unsaved work; the user is shown a recovery summary (named command groups, timestamps) and chooses restore/discard per document.
3. **Torn save:** rename-path saves are atomic by construction; append-path saves write a journal intent record first, enabling detection and byte-exact truncation rollback of an incomplete increment on next open (the `/Prev` chain guarantees the prior revision is intact).
4. **Sidecar hygiene:** journals live in per-user app state (never beside the document by default — avoids litter and leaks), are encrypted when the source document is encrypted, and are deleted on clean save/close.
5. **Crash reporting:** minidump capture (Breakpad/Crashpad-class) is local-only; submission is a user-initiated act showing the exact artifact contents (ADR-020 privacy model applies).

**Alternatives considered.** (a) Timed full-document autosave copies (incumbent pattern): O(file) I/O, races with large documents, and silently multiplies confidential copies on disk. Rejected. (b) No journal, rely on frequent saves: pushes durability responsibility onto users — the failure mode behind decades of lost work. Rejected. (c) Sidecar beside the document: interop litter, leaks work-in-progress into shared folders/VCS. Rejected.

**Trade-offs.** Journal replay must be versioned with the command schema (a real maintenance obligation across releases); encrypted-journal handling adds key-lifecycle code.

**Consequences.** "You cannot lose more than ~2 seconds of work" becomes a testable claim (crash-injection tests, ADR-022); recovery UX is calm because the substrate is exact.

**Future considerations.** Journal format stability could later permit "resume session on another machine" via user-controlled sync — only under an explicit ADR passing the trust test.

---

## ADR-022 — Testing Philosophy

**Status:** Accepted

**Context.** The spec is ~1,000 pages and reality deviates from it constantly; correctness is corpus-shaped, not unit-shaped [R2, R4]. pdf.js's reftest infrastructure, hayro's corpus-first discipline, veraPDF's conformance suites, and PDFium's fuzzing regime are the proven patterns. Our differentiators (redaction proof, signature validation, crash recovery) require test types most projects never build.

**Problem statement.** Define the test taxonomy, its gating role, and the infrastructure commitments before the first feature exists — because retrofitted test culture is the one thing that cannot be incrementally saved.

**Decision.** Six mandatory strata, all CI-gated (ADR-029):
1. **Unit + property tests** (proptest-class) for all pure logic — serializers round-trip, normalizers, geometry math; property tests are required for any parser/writer pair (parse∘write∘parse = id on the model).
2. **Corpus regression:** a versioned corpus (seeded from pdf.js, PDFBox, veraPDF/Isartor, PDF Association suites, plus our triaged bug-report files under contributor license) driving deterministic rasterization with perceptual image-diff against goldens; every rendering PR shows its diff set for human triage — the pdf.js model, adopted nearly verbatim.
3. **Differential testing:** the same corpus rendered/extracted through secondary oracles (hayro backend, MuPDF tools externally, Acrobat manually for triage) with divergence dashboards — our engine-boundary design makes this cheap (ADR-005).
4. **Fuzzing as a permanent organ:** cargo-fuzz targets for every input surface (COS parser, filters, xref recovery, protocol, journal replay, WIT boundary) running continuously; OSS-Fuzz enrollment at public launch; every fuzz crash becomes a corpus file and a regression test.
5. **Fault-injection & recovery tests:** scripted worker kills mid-render, coordinator kills mid-mutation, truncated-save simulation — asserting the ADR-021 guarantees literally.
6. **Conformance & interop:** veraPDF validation of everything our writer emits; an interop matrix (annotations/forms authored by us, opened in Acrobat/Foxit/browser viewers and vice versa) executed per release train — because interop pain #10 is fixed only by testing against the incumbents, not the spec.
Policy: no feature merges without tests in the strata it touches; flaky tests are release-blocking defects, not annotations.

**Alternatives considered.** (a) Conventional unit+integration pyramid only: cannot represent corpus-shaped correctness; rejected as sufficient. (b) Manual QA cycles: does not scale to a contributor project and decays; rejected as a primary mechanism. (c) Golden-image tests without human triage tooling: produces rubber-stamped diff churn; the triage UI is part of the commitment, not optional.

**Trade-offs.** Corpus infrastructure (storage, licensing hygiene, diff-triage tooling) is a real sub-project funded before features; differential dashboards demand maintenance; strict gating slows merges by design.

**Consequences.** Fidelity becomes a monotonically guarded property; contributors get objective acceptance criteria; the corpus itself becomes a community asset and moat.

**Future considerations.** A public "compatibility score" page generated from the interop matrix — marketing produced by CI.

---

## ADR-023 — Benchmarking Strategy

**Status:** Accepted

**Context.** ADR-001 makes performance contractual; contracts require measurement. Foxit's decay [R3] shows unmeasured performance regresses monotonically under feature pressure.

**Problem statement.** Turn the published budgets into regression-gated, reproducible, public numbers across three platforms for a decade.

**Decision.**
1. **Two benchmark tiers:** micro (criterion-class: parser throughput, filter decode, tile rasterization, journal commit) and **macro scenario benchmarks** — scripted end-to-end runs on reference documents (cold start→first page; open 2,000-page file→scroll script→frame-time distribution; incremental save latency; OCR pages/minute; memory ceiling under the scroll script) executed on **fixed dedicated hardware** per OS, because cloud-runner variance destroys gating validity.
2. **Budgets as assertions:** each macro scenario carries a budget from ADR-001 and a regression tolerance; exceeding either fails the release gate (main-branch merges get trend alerts; release branches get hard gates — pragmatic two-level enforcement).
3. **Percentiles, not means:** frame-time and latency budgets bind at p95/p99 — jank lives in the tail.
4. **Public dashboard:** per-release numbers published automatically; the honesty value applied to ourselves.
5. Reference document set is versioned with the corpus (small/huge/scan-heavy/transparency-heavy/form-heavy classes) so numbers stay comparable across years.

**Alternatives considered.** (a) Benchmarks run ad-hoc "when performance work happens": guarantees decay between efforts. Rejected. (b) CI-cloud benchmarking with statistical noise correction: tempting, but gating on noisy infrastructure yields either false blocks or widened tolerances that hide real regressions; dedicated hardware is cheaper than the alternative's dysfunction. Rejected for gating (retained for smoke trends). (c) Budgets on means: hides jank; rejected.

**Trade-offs.** Dedicated benchmark hardware is ongoing cost and single-point maintenance; hard gates will occasionally block urgent releases pending optimization — that is the mechanism working.

**Consequences.** "Faster than Acrobat" becomes a reproducible artifact, not a slogan; performance regressions surface at PR granularity with a culprit trail; opportunity O1 is institutionalized.

**Future considerations.** Community-contributed benchmark replays from real (sanitized) workloads; energy-use metrics once tooling matures.

---

## ADR-024 — Repository Layout

**Status:** Accepted

**Context.** A dual-language codebase, a large binary corpus, vendored engines, and a ten-year contributor horizon each stress repository topology differently.

**Problem statement.** Choose mono- vs multi-repo and the top-level structure so that atomic cross-boundary change, corpus scale, and contributor onboarding all work.

**Decision.** A **single primary monorepo** for application + core + shell + CLI + plugin SDK + docs, with exactly three satellite repositories:
1. **Main monorepo**, top level: `core/` (Rust workspace, ADR-025), `shell/` (Qt project, ADR-026), `cli/`, `plugin-sdk/` (WIT worlds + language bindings + test-kit), `docs/` (including `docs/adr/`, one file per ADR, this constitution's home), `tools/` (triage UI, corpus tooling, benchmark harness), `third_party/` (vendored engines with provenance manifests, ADR-028).
2. **Corpus repository** (separate; LFS/object storage-backed): test documents + goldens — kept out of the main repo so clones stay light and file licensing is administered in one place with its own contribution agreement.
3. **Benchmark-results repository:** append-only public numbers (ADR-023).
4. **Website/registry repo** when those exist.
Rules: protocol crate, WIT worlds, and bridge live in the monorepo so any cross-boundary change is one atomic PR; no nested submodule chains (vendoring by subtree/script with lockfiles instead).

**Alternatives considered.** (a) Multi-repo per component: forces lockstep-versioning ceremonies for the protocol/bridge/API surfaces that change together constantly in early years. Rejected. (b) Corpus inside the monorepo via LFS: couples every contributor's clone to gigabytes and every corpus-license question to the code repo. Rejected. (c) Separate shell repo to "enforce" the boundary: the boundary is enforced by the protocol and review policy (ADR-004/026), not geography; separation would tax every feature. Rejected.

**Trade-offs.** Monorepo CI must be path-aware to keep feedback fast (ADR-029); the corpus repo needs its own access/licensing governance.

**Consequences.** One PR = one reviewable cross-cutting change; ADRs live beside the code they govern; onboarding is one clone + one artifact fetch.

**Future considerations.** If the core crates gain significant external embedders, publishing cadence to crates.io gets its own ADR; the monorepo remains the source of truth.

---

## ADR-025 — Rust Workspace Layout

**Status:** Accepted

**Context.** Crate boundaries are the enforcement mechanism for the architecture: dependency direction between model, engine, and shell-facing layers must be compiler-checked, not convention-checked. Build times over a decade depend on decomposition now.

**Problem statement.** Define the workspace's crates, their dependency direction, and the rules that keep boundaries real.

**Decision.** One Cargo workspace under `core/`, layered strictly (dependencies point downward only):
- **Foundation:** `pdf-cos` (object store, xref, filters, repair/leniency ledger); `pdf-types` (shared primitives: geometry, ids, revision keys); `diagnostics` (tracing setup, redaction wrapper types, ADR-020).
- **Model:** `pdf-model` (semantic façades, Commands, journal — ADR-006/013); `pdf-write` (serializers: incremental + rewrite paths, ADR-012).
- **Engine seam:** `engine-api` (the capability traits, ADR-005 — depends only on foundation); `engine-pdfium`, `engine-hayro` (backends, each isolatable behind features).
- **Services:** `render-pipeline` (ADR-007), `text-extract`/`search` (ADR-019), `jobs` (ADR-009), `ocr-bridge` (ADR-018), `sign` (PAdES stack), `plugin-host` (ADR-014/15), `sandbox` (per-OS worker confinement, ADR-008/016).
- **Composition:** `coordinator` (wires services per ADR-010), `worker-main` (Z1 binary), `protocol` (command/event types, ADR-004), `ffi-bridge` (the single cxx boundary), `cli`.
Rules: `engine-api` must not know any backend; `pdf-model` must not know any engine; nothing below `protocol` may reference Qt concepts; `unsafe` outside `sandbox`, `ffi-bridge`, and backend glue requires a lint-allow with written justification (ADR-027); crate-level docs are mandatory and CI-checked.

**Alternatives considered.** (a) Few large crates ("core", "app"): faster to start, boundaries decay into convention, incremental compile suffers exactly when the codebase is largest. Rejected. (b) Maximal micro-crates (one per module): version/README ceremony without enforcement gain beyond the layer boundaries above. Rejected. (c) Backends in separate repos: breaks atomic engine-trait evolution during the years the trait is still learning. Rejected (revisit if backends stabilize and attract external maintainers).

**Trade-offs.** Layering will occasionally force plumbing types through `pdf-types` rather than convenient upward references — deliberate friction; feature-flag matrices (engines × scripting × platform sandboxes) need CI coverage discipline.

**Consequences.** The architecture diagrams and the workspace graph are the same artifact; a violation is a compile error; the headless SDK (opportunity O8) is `coordinator`+below, proven by `cli` daily.

**Future considerations.** `sign` may split validation/creation when the validation-worker split (ADR-016 futures) lands.

---

## ADR-026 — Qt Shell Project Structure

**Status:** Accepted

**Context.** The shell's constitutional constraints: thin (≤15% of code), stateless with respect to documents, boring on purpose (ADR-003/004). Structure must make violations visible in review.

**Problem statement.** Organize the C++/Qt project so domain logic physically has nowhere to live, and shell contributions stay approachable to Qt-experienced contributors who know no Rust.

**Decision.** `shell/` as a CMake project (Qt 6, Widgets), structured by responsibility:
- `bridge/` — the sole cxx counterpart (ADR-004): dispatcher marshaling events to the UI thread, command submission, shared-memory tile mapping. Two-reviewer rule applies here.
- `app/` — application lifecycle, sessions, settings storage (UI preferences only — document state is core-owned), single-instance/file-association handling.
- `chrome/` — main window, docking, menus/toolbars/shortcut registry (the shortcut map is a versioned data file: the interface-stability contract, ADR-001 value 3, as an artifact under diff review).
- `canvas/` — viewport widget, GPU tile compositor, input→command translation, selection/annotation overlays rendered from core-provided geometry.
- `panels/` — bookmarks, thumbnails, layers, comments, diagnostics (ADR-020), each a dumb view over protocol events.
- `dialogs/` — print (QPrinter integration), preferences, consent prompts (ADR-016 broker UX).
- `a11y/` — QAccessible surface mapping core structure events to the platform tree.
- `platform/` — per-OS integration (shell previews, jump lists, services).
Rules: no file in `shell/` may parse PDF syntax, include engine headers, or store document truth (a canvas may cache *pixels and geometry*, never objects); UI text is translation-wrapped from day one; widget code carries QTest coverage for input→command translation (the shell's one testable responsibility, wired into ADR-022 CI).

**Alternatives considered.** (a) Feature-sliced structure (annotations/, forms/ each with UI+logic): invites logic to accrete beside UI — the decay path this ADR exists to block. Rejected. (b) QML for panels within a Widgets app: mixed-paradigm maintenance for marginal gain now (revisit per ADR-003). Rejected. (c) Auto-generated UI from protocol schemas: right idea for *plugin* panels (ADR-014), too rigid for first-party ergonomics. Adopted only for plugin surfaces.

**Trade-offs.** Strict statelessness means some UI conveniences (e.g., instant optimistic edits) must round-trip the core — asynchronous UX patterns are mandatory (already required by ADR-004).

**Consequences.** The shell is learnable in an afternoon; the shortcut/stability contract is enforceable by reviewing one file's diffs; future shell replacement (ADR-003 futures) has an inventory: this directory listing.

**Future considerations.** If plugin-declared panels grow rich, `panels/` gains a schema-renderer submodule shared with the plugin surface.

---

## ADR-027 — Coding Standards and Review Policy

**Status:** Accepted

**Context.** Ten years × many contributors × two languages × a security-critical domain: standards must be mechanical wherever possible (tools reject, humans review judgment), and the few human-judgment zones must be named.

**Problem statement.** Fix the standards, their enforcement mechanism, and the escalated-review zones.

**Decision.**
1. **Rust:** stable toolchain, pinned per release train with a published MSRV policy (trailing ~2 stable versions); `rustfmt` (default profile) and `clippy` at pedantic-leaning configuration are CI-blocking; `#![forbid(unsafe_code)]` in every crate except the named exceptions (ADR-025), where each `unsafe` block requires a `// SAFETY:` proof comment and appears in a generated audit index; public items require doc comments (missing-docs lint on).
2. **C++ (shell):** C++20, Qt style, `clang-format` + `clang-tidy` (modernize + bugprone + cert profiles) CI-blocking; no raw owning pointers (Qt parent ownership or smart pointers); no exceptions across the bridge; RAII everywhere.
3. **Cross-cutting:** error handling is typed and propagated (no silent fallback — leniency must ledger, ADR-006); public protocol/WIT changes require a compatibility note in the PR; commit messages follow a conventional format feeding changelog automation (ADR-030).
4. **Review policy:** default one qualified reviewer; **two reviewers, at least one from a named owners list, for:** `ffi-bridge`+`bridge/`, `sandbox`, broker interfaces, `sign`, `pdf-write` serializers, journal replay, and any `unsafe` diff. ADR-affecting changes require a superseding/amending ADR in the same PR.
5. Contributor experience is a standard too: a one-command dev setup (prebuilt engine artifacts), CONTRIBUTING with the layer map, and "good first issue" gardening are maintained deliverables, not afterthoughts.

**Alternatives considered.** (a) Style by culture, not tools: decays with contributor turnover; rejected. (b) Blanket two-reviewer policy: throughput death for a volunteer project; risk-scoped escalation targets review where consequence lives. (c) Nightly Rust for expressiveness: toolchain churn tax across a decade outweighs features; rejected.

**Trade-offs.** Pedantic lint walls frustrate drive-by contributors (mitigated by pre-commit tooling and CI autofix suggestions); owners lists create review bottlenecks that must be staffed consciously.

**Consequences.** Codebase texture stays uniform across years and authors; the `unsafe` audit index makes the memory-safety claim inspectable; security-critical surfaces cannot change casually.

**Future considerations.** Adopt cargo-mutants or similar mutation testing for the serializer/journal zone once runtimes permit.

---

## ADR-028 — Dependency Policy

**Status:** Accepted

**Context.** pdftk died of a dependency (GCJ) [R3]; supply-chain attacks target exactly our kind of trusted tool; licenses must stay coherent (GPLv3 application, permissive core seams, LGPL Qt, BSD PDFium).

**Problem statement.** Govern what may be depended on, how it's verified, and how it's carried for a decade.

**Decision.**
1. **License allowlist, CI-enforced** (cargo-deny + shell equivalent): permissive (MIT/Apache/BSD/Zlib/MPL-2.0) freely within policy; LGPL dynamic-link only (Qt); GPL acceptable only where the component is process-isolated or the consuming artifact is itself GPL-distributed; **AGPL forbidden in anything we link** (MuPDF/Ghostscript remain external oracles only). Every third-party addition records provenance in `third_party/` manifests.
2. **Dependency tiers:** Tier 1 (load-bearing: engines, wasmtime, tantivy, tracing, cxx, Qt) requires a written adoption note — health, governance, bus factor, exit strategy — reviewed like an ADR; Tier 2 (utility crates) requires cargo-vet/audit trail; transitive bloat is monitored (dependency-count and build-time budgets per crate).
3. **Vendoring policy:** engines and any Tier 1 C/C++ are vendored with pinned upstream refs, local patches maintained as a rebased series, and an upstream-first rule (patches must be submitted upstream unless documented why not).
4. **Lockfiles are law:** all builds, including CI and release, are lockfile-exact; updates land as reviewed PRs (grouped, automated proposal, human merge), never silently.
5. **Exit strategies are part of adoption:** every Tier 1 dependency's note names the migration seam (engine trait, `OcrEngine`, WIT contracts, protocol crate) that contains it — the pdftk lesson institutionalized.

**Alternatives considered.** (a) Minimal-dependency purism (write everything): forfeits proven, fuzzed code (compression, crypto) for NIH risk; rejected. (b) Laissez-faire crates.io grazing: supply-chain and license entropy; rejected. (c) Forking Tier 1 dependencies preemptively: maintenance mass without benefit while upstreams are healthy; the vendor+patch-series model captures the option without the cost.

**Trade-offs.** Adoption notes and vet trails slow "just add the crate" velocity (deliberate); the allowlist occasionally excludes technically best-in-class libraries (documented exceptions path: an ADR).

**Consequences.** The SBOM is generatable and honest (feeds reproducible builds, ADR-029); no dependency can die and take us by surprise — every one has a named containment seam.

**Future considerations.** Reproducible-build attestation of vendored toolchains (PDFium's build ecosystem) is the hardest remaining supply-chain gap; tracked as standing work.

---

## ADR-029 — CI/CD Strategy

**Status:** Accepted

**Context.** The constitution's enforcement arm: budgets (ADR-023), test strata (ADR-022), standards (ADR-027), dependency law (ADR-028), and reproducibility (ADR-016/020 trust claims) all bind only if CI binds them. Dual-language monorepo + 3 OSes + vendored Chromium-style engine = real build-engineering mass.

**Problem statement.** Define pipeline stages, gating levels, artifact strategy, and the reproducibility commitment.

**Decision.**
1. **PR pipeline (fast, path-aware):** format/lint (both languages) → affected-crate unit/property tests → protocol/WIT compatibility check → targeted corpus subset with image-diff artifact for reviewer triage → shell QTest suite; target wall-clock ≤ 20 minutes via prebuilt engine artifacts (engines rebuild only when `third_party/` changes) and aggressive caching.
2. **Merge pipeline (main):** full corpus regression on all 3 OSes, differential-oracle dashboards, fuzz smoke (bounded corpus replay), fault-injection suite, benchmark *trend* run (alerting, non-gating).
3. **Release pipeline:** everything above + hard benchmark gates on dedicated hardware (ADR-023) + veraPDF conformance of writer outputs + interop matrix + **reproducible-build verification** (two independent builders must produce bit-identical artifacts; divergence is release-blocking) + signed artifacts (per-platform code signing; SLSA-style provenance attached) + SBOM publication.
4. **Continuous organs:** fuzzing runs perpetually off-pipeline with auto-filed crashes; nightly builds publish for the opt-in channel (ADR-030).
5. Infrastructure is code-reviewed like product code; a broken main gate is an all-hands stop-the-line event, not a bypass candidate.

**Alternatives considered.** (a) Full corpus on every PR: hours-long feedback kills contribution; subset+full-on-merge is the standard resolution. (b) Gating benchmarks in cloud CI: rejected in ADR-023 (noise). (c) Best-effort reproducibility ("we try"): an unverifiable trust claim is a marketing liability; either verified or not claimed — we verify.

**Trade-offs.** Reproducible builds constrain toolchain choices and cost real engineering (timestamps, path embedding, PDFium's build); path-aware pipelines add CI complexity that itself needs maintenance.

**Consequences.** Every constitutional value has an enforcement point a contributor can see fail; releases carry provenance an enterprise auditor can check; "trust by construction" becomes literal.

**Future considerations.** Third-party build attestation (independent org rebuilding releases) once the project has the community mass to sustain it.

---

## ADR-030 — Release Strategy

**Status:** Accepted

**Context.** User research: interface churn and forced change are top-tier pains; enterprises need predictability (pain #7); plugin authors need API stability (ADR-014/015); yet a young project needs iteration speed. These tensions resolve by channel, not compromise.

**Problem statement.** Define cadence, channels, versioning, support windows, and the mechanics of the interface-stability contract.

**Decision.**
1. **Time-based release trains** (fixed cadence, e.g., every 12 weeks): features ship when ready, trains ship on time — no feature ever slips a date, dates never slip for features.
2. **Three channels:** Nightly (opt-in, from main), Stable (the train), **LTS** (annual designation, security + critical fixes for ≥ 24 months) — the enterprise deployment answer, paired with policy-controlled packaging (MSI/GPO, PKG/MDM, repo/Flatpak) and *no forced upgrades ever*.
3. **Versioning is per-contract:** application version (calver-style train id) is informational; the **protocol, plugin API (WIT worlds), and journal/file-sidecar formats each carry independent semver** with published deprecation policy (≥ 2 trains of overlap, ADR-015).
4. **The UI stability contract, mechanized:** the shortcut/menu registry (ADR-026) and workflow-level behaviors are versioned data; any change ships as a new *UI profile version* — users and org policy pin profiles; the previous profile is supported indefinitely for shortcuts/menu taxonomy and ≥ 4 trains for layout changes, with in-product diff notes ("what changed and how to revert"). Removal of a profile toggle requires a superseding ADR with public comment — the anti-Acrobat-2023 clause, constitutionally.
5. **Updates:** the updater checks only on user-visible schedule, changes nothing without consent, and never bundles offers; release notes are honest changelogs generated from commit convention (ADR-027) plus human-written highlights.
6. Security releases ride an expedited out-of-train path to all channels simultaneously.

**Alternatives considered.** (a) Feature-based releases ("ship when X is done"): schedule chaos, pressure to merge unready work; rejected. (b) Rolling-release only: hostile to enterprise validation cycles and to the stability contract; rejected as the *only* channel (Nightly preserves its virtues). (c) Single semver for the whole application: conflates contracts with marketing and forces false major bumps; per-contract versioning is more honest and more useful. (d) Auto-updating silently "for security": violates consent values; expedited *offered* updates + LTS backports achieve the security goal without the coercion.

**Trade-offs.** LTS backporting is a permanent engineering tax that grows with codebase size; UI profile support multiplies UX test surface (bounded by the profile-count policy above); time-based trains occasionally ship "thin" releases (acceptable — honesty over theater).

**Consequences.** Enterprises can write us into multi-year plans; plugin authors get contractual notice; the project's central UX promise — *your muscle memory is an API* — has an enforcement mechanism with a paper trail.

**Future considerations.** A signed plugin registry with its own release governance; distribution-channel expansion (store submissions) gets a dedicated ADR when scheduled, constrained by the no-forced-change clause.

---

*End of baseline constitution, ADR-001 … ADR-030. Amendments proceed by superseding ADR only.*
