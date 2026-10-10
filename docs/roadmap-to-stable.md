# Roadmap to a stable first release

**Status:** working plan, derived from the project's own documents. Written 2026-10-10, revised the same day after reading `docs/system-design-specification.md` §14, `docs/product-requirements-document.md` §8 and §16, `docs/adr-constitution.md` ADR-030, `docs/release-gates.md` and `docs/superpowers/specs/2026-08-23-render-pipeline-bypassed-design.md`. Status claims come from `docs/milestone-exit-tracker.md` and the baseline run recorded there.

**Goal:** release stable, not first. Precedence on conflict: ADR → SDS → PRD → UI/UX → Implementation Guide. Where a document already decides something, this plan follows it and does not reopen it.

## What the documents already decide

| Question | Answer | Source |
|---|---|---|
| What is release 1? | "A viewer and toolkit professionals trust": viewing, search, selection, extraction; bookmarks, thumbnails, layers, attachments; disclosed repair; annotations and review with interop; page organization (merge, split, extract, insert, delete, reorder, rotate, crop); open encrypted files; basic printing; export to images, text and HTML; incremental save, undo, autosave, recovery; revision-history view and sanitize; accessibility; CLI with GUI parity | PRD §16.2 |
| Where do forms, redaction, signatures, OCR, stamping, PDF/A, compare, plugins go? | Version 2 and 3. They may exist in code, but must be disclosed as partial (ROAD-2) and a release must not ship what violates §8.2 (SCOPE-3) | PRD §16.1, §16.3, §16.4, §8.4 |
| How are releases cut? | Each milestone is a releasable build. Time-based trains; tags `v0.<milestone>.0-rc.N`; changelog cites IDs and lists honest non-claims; **a human cuts public tags and GitHub Releases**. Confinement is called "advisory", never "fully sandboxed", until the review package is signed | SDS §14 invariants, ADR-030, `release-gates.md` |
| Which render path is canonical? | The coordinator's (SDS §6, ADR-007, ADR-011). The code does the opposite today. Code follows the documents, so the shell is to be routed through the coordinator loop. This is a bridge change: drafted by an agent, reviewed by a human with two reviewers (AI-6, FFI-6, CR-Z) | README "authoritative; code follows them", IG §3 and §14, render-pipeline note, option A |
| Order of work | Dependency spine: overlay/model → save/undo/recovery → rendering → features. No editing feature before the M3 fault gate (satisfied) | SDS §14 |

## Working rules (anti-hallucination)

Source: `IMPLEMENTATION_GUIDE.md` §13.

1. A status claim cites the command and its output, or says "not measured" [T-13, AI-8].
2. Check a file, function or flag exists (Grep/Read) before naming it. Never cite from memory.
3. Cite only requirement IDs that exist; `tools/check-citations` stays green [G-1, AI-2].
4. A new test or gate is made to fail once against a broken implementation, and that is recorded [T-11].
5. Tests assert the artefact (saved file, pixels, extracted text), not that a function ran [T-10].
6. Security-critical code (redaction, signatures, sandbox, crypto, FFI) is drafted only; a human owner reviews and merges [AI-6, CR-Z].
7. If a spec is silent, stop and ask; do not fill gaps from general PDF knowledge [AI-1].
8. One small PR per change; the tracker row moves in the same PR [AI-10].
9. Agents prepare docs and verify gates; humans cut tags [`release-gates.md`].

## Phase 0: Establish the truth (nearly done)

Done, with evidence in the tracker row "Gate run of 2026-10-10": tools installed and verified; `cargo test --workspace` 761 passed, 0 failed; corpus-diff, a11y, window-length, dead-API, citation and UI-registry gates green; shell built and 4/4 ctest; `main` CI green.

Still open:

| # | Task | Done when |
|---|---|---|
| 0.1 | Two uncommitted edits in `pdf-cos/src/scan.rs` and `xref.rs` (a parameter rename, a stray blank line) | Committed or discarded; `git status` clean |
| 0.5 | Open a PDF by hand in `shell\build-verify\Release\pdf-platform.exe` | Result recorded; only a human can see the window |
| 0.6 | README status table matches the tracker (M7 is not implemented; M8, M10, M11 partial) | README and tracker agree row by row |
| 0.8 | veraPDF writer gate (downloads an installer) | Run once with approval, result recorded |

## Phase 1: Clear the PR queue

| PR | Action | Gate |
|---|---|---|
| #86 dead-API gate | Review, merge | CI green; gate observed failing once [T-11] |
| #70 bounded page-text cache | Review, merge | Touches the bridge: two reviewers [FFI-6] |
| #63 plugin module loading | Review, merge or park | Plugins are V3 scope (PRD §16.4); must stay disclosed as partial |
| #59 plugin limits, #42 signature xref, #48 optimizer confinement | Drafts; human review | Security paths [AI-6]; not on the V1 critical path |

## Phase 2: Complete version 1 (PRD §16.2)

Gaps found by comparing PRD §16.2 with the tracker and a search of `core/` and `shell/` (the search is not exhaustive: it found no printing, no revision-history view, no crop command, and `export-text` as the only export).

| # | Item | Requirement | Notes |
|---|---|---|---|
| 2.1 | Route the shell through the coordinator's render loop | FR-NAV-1 (facing layouts), SDS §6.9 prefetch, GR-7 bounded tile cache | Bridge change, human-gated. Update `MET-PERF-3` to time the real path |
| 2.2 | Layers panel shows names | FR-VIEW-4 per tracker | `get_layers_impl` forwards only counts; FFI-gated |
| 2.3 | Printing | FR-PRINT-1, 2, 3, 5 | None found in code. Native dialog, ranges, scaling, accessible |
| 2.4 | Export images and HTML (text exists) | FR-EXPORT-1, 3 | CLI first, then GUI [SDS §14 parity] |
| 2.5 | Revision-history view and sanitize | FR-VER-1, FR-VER-2 | Sanitize discloses what is removed |
| 2.6 | Crop; CLI parity for the V1 operations | FR-ORG family, FR-CLI-1 | Check which operations the CLI lacks before building |
| 2.7 | Tagged-document reading order and text export | FR-A11Y-1, FR-A11Y-3 | `has_structure` is hard-coded false today |
| 2.8 | Encrypted documents: save works or is refused clearly in the UI | FR-SAVE, FR-SEC-1 | Crypto: human-gated. Refusal exists in `save_incremental` |
| 2.9 | Incremental save onto xref-stream documents writes a hybrid | FR-SAVE-1 | Verify with `qpdf --check` |
| 2.10 | Review findings: revocation hook swallows a poisoned mutex; `publish_verified` rename can replace a user file | GR-8, PRIN-2 | Security paths: human-gated |

## Phase 3: Version 1 exit evidence

| # | Item | Source | Who |
|---|---|---|---|
| 3.1 | 2,000-page scan-heavy fixture and p95 benches on pinned reference hardware | SDS §14 M1, B-3, MET-GOV-1 | Hardware: human |
| 3.2 | Screen-reader pass | `docs/a11y-audit-checklist.md` | Human |
| 3.3 | Real-world corpus for open and open-edit-save (start with the pdf.js corpus) | MET-FEAT-1, 4; tracker notes the corpus is synthetic | Agent can wire it |
| 3.4 | Acrobat/Foxit open of an XFDF round trip | `release-gates.md` | Human, optional |
| 3.5 | Sandbox stays "advisory" in all wording until the review package is signed | `release-gates.md` | Human |
| 3.6 | Release candidate: green CI on the release commit, changelog with honest non-claims, annotated tag `v0.<milestone>.0-rc.N` | `release-gates.md` RC procedure | Agent prepares; human tags |

## Version 2 and beyond (not in the V1 release)

Forms with JavaScript, verifiable redaction, signature validation and signing, OCR, watermarks and Bates, PDF/A, compare, image editing (PRD §16.3); then text editing, hardware signing, plugins, prepress (PRD §16.4). Several exist in partial form. Each must be labelled partial in the V1 changelog [ROAD-2].

## Using PdfCraft

As an oracle and test source: open its output in this viewer and the reverse, compare redaction and signature results, borrow the corpus sweep idea. Importing its code needs a DEP-1 review and passes through the gates in this plan first. Its logos and brand assets are not open source.

## Skills and tools

- **ponytail:** smallest change, finish callers and tests. `/ponytail-review` per PR; `/ponytail-audit` once after Phase 0; `/ponytail-debt` for `shortcut:` comments.
- **code-review** and **security-review:** before merging anything on the AI-6 list.
- **simplify:** after behaviour is covered by tests.
- **run:** launch the app and confirm a change in the real shell.
