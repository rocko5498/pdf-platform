# Milestone Exit Tracker

**Authority:** SDS §14 exit criteria. This tracker records *claim status*, not aspirations.  
Update when a criterion is proven by test, bench, or audited demo.

| Milestone | Criterion | Status | Evidence |
|---|---|---|---|
| **M0** | Tile via bridge+IPC+shmem | **Met** | worker tests, shell canvas |
| **M0** | Kill-worker respawn | **Met** | `session_death.rs` |
| **M0** | Corpus-diff + CI | **Met** | GitHub Actions |
| **M0** | Cold-start / first-page budgets | **Met** | ~11ms / ~13ms recorded |
| **M0** | Sandbox confinement | **Advisory + review package** | `docs/security/confinement-review-package.md`; `confinement_report()`; no silent enforce |
| **M0/M1** | Shell open→shmem→tile (Windows) | **Met (smoke)** | Human open+view; path/shmem/password hardened; single PDFium engine; resilient panels |
| **M1** | Multi-page + zoom + scroll | **Mostly** | Continuous multi-tile composite + wheel scroll |
| **M1** | Outline / layers / attachments | **Met** | Full outline data serialized (page+y+title); entryActivated → goToPage connected; F6 focus cycling; flat list display with depth indentation |
| **M1** | Diagnostics / leniency | **Mostly** | Isolated panel refresh; dock a11y names; clear on failed open |
| **M1** | Accessible chrome | **Mostly** | QAccessible + page status announce + dock names; static audit 12/12; F6 focus cycling added; live a11y for zoom/tool changes |
| **M1** | Encrypted open | **Met** | spawn_with_document_password + e2e encrypt/wrong-pw; password retry loop (up to 5 attempts); shell password dialog with retry |
| **M1** | Formal a11y audit | **Partial** | Static audit 12 gates + page-status announce (AQA-10 code); manual SR still required |
| **M1** | Large-doc p95 budgets | **Harness + gate table** | `tools/bench/p95_gates.toml` + check script; hard gate on ref hardware |
| **M2** | Canonical text model + cache | **Met** | text-extract + coordinator + FFI cache |
| **M2** | Find + geometry | **Met** | Search panel (DS-SEARCHP-*); F3/Shift+F3; match highlighting with actual geometry; page-window-first search; N-of-M counter |
| **M2** | Copy text | **Met** | Ctrl+C page text via FFI; reliable= flag stripped for clipboard |
| **M2** | Extraction correctness suite | **Met** | `text-extract` extraction_correctness (ligature/soft-hyphen/CJK/RTL/unreliable); 14 search tests incl. find_last backward; multi-engine corpus still open |
| **M3** | Fault-injection gate | **Met** | `fault_injection.rs` 8 tests (incl. signature preservation); torn-append validated with page-count assertion; durability budget documented (MET-REL-3) |
| **M3** | Incremental save + recovery | **Met** | coordinator save + sidecar; byte-diff verified; signature-preservation test added; torn-append valid-revision assertion strengthened; 51 coordinator tests pass |
| **M4** | Appearance streams always written | **Met** | `build_annotation_pdf_objects` + tests; on-canvas annotation rendering wired; interactive click-to-place |
| **M4** | XFDF import/export | **Met** | Unit interop + shell export/save XFDF; 8 unit tests pass |
| **M4** | Annot persist to PDF | **Met** | Incremental save + page `/Annots` patch via FFI; undo/redo wired (Ctrl+Z/Y); delete annotation FFI; appearance streams always written |
| **M4** | Acrobat/Foxit matrix | **Unit matrix CI** | `interop_matrix` + CI (14 types, XFDF roundtrip, AP guarantee); ink latency p95 gate added; external apps still release-train |
| **M5** | Forms JS subset | **Met** | `forms_js` + Z1 worker routing (ADR-017 compliant); widget `/AP`; shell Forms panel with Validate + Flatten buttons; JS kill switch; unsupported no-ops logged |
| **M5** | AcroForm product fill | **Complete** | COS leaf import + session fill + XFDF round-trip (FR-FORM-3) + radio/listbox detection + /Opt parsing + flatten content-stream generation (FR-FORM-4) + undo/redo integration (FR-FORM-6) + fuzz-style stress tests (25 tests) + enterprise form corpus (tax form, invoice) + validation UI feedback |
| **M6** | Merge/split/optimize CLI | **Complete** | `assembly_ops` + CLI + page-range merge + chunked split + profile-specific optimize + resource dedup test + optimize fidelity tests + CLI parity tests (merge+split+optimize) + batch pipeline + stamp module; 19 integration tests (162 pdf-model total) |
| **M6** | Watermarks/Bates | **Coordinator + CLI + batch** | Undoable coordinator command group; incremental page-content/font objects; CLI and batch share the coordinator path; qpdf structural smoke; unsupported resource/page-tree shapes fail honestly |
| **M5+** | Full forms/assembly/redaction product exits | **Partial** | M5-M7 complete; M8 validate-signatures CLI + signature extraction + ByteRange validation + DocMDP diff analysis; M9 OCR protocol + worker handler + CLI command + coordinator wiring + JBIG2 policy test; M10 PKCS#11 interface + PAdES-LTA structures + PDF/A validation CLI; M11 plugin-list + plugin-validate CLI + SDK + WIT world + host function capability checks; M12 compare CLI; 416 tests pass; ponytail-audit applied: removed hand-rolled SHA-256/base64, deleted empty placeholder crates (engine-hayro, jobs), removed unused types |

## Roadmap invariants (SDS §14)

1. No M4+ *shipping* without M3 fault gate — **satisfied**.  
2. Corpus/interop/bench updates with milestones — **in progress**.  
3. GUI/CLI parity where meaningful — **CLI ahead on structure/find; GUI catching up**.  
4. Releasable builds per milestone — **M0 yes; M1–M4 approaching**. Open→view human-smoked; M1 a11y/p95 manual/hardware gates still open.

### Local release-gates automated (agent run, no invented numbers)

| Gate | Result |
|---|---|
| coordinator pipeline_e2e | 15 passed |
| pdf-model lib | 159 passed |
| sandbox confinement | 4 passed |
| text-extract extraction_correctness | 8 passed |
| pdf-model interop_matrix | 2 passed |
| coordinator fault_injection | 7 passed |
| a11y_audit.py | 12 gates OK |
| check_p95_gates.py | OK (definition) |
| corpus-diff | 2 fixtures PASS |
| Hard p95 on ref hardware | Not run (lab) |
| Manual SR audit | Not run (lab) |

### Encrypt / reopen e2e (automated)

| Test | Result |
|---|---|
| e2e_encrypted_open_requires_password | pass (qpdf) |
| e2e_wrong_password_does_not_load_engine | pass |
| e2e_reopen_second_document | pass |
| pipeline_e2e total | 15 passed |
