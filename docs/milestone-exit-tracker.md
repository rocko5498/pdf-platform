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
| **M1** | Multi-page + zoom + scroll | **Mostly** | Continuous multi-tile composite + wheel scroll |
| **M1** | Outline / layers / attachments | **Mostly** | Live worker queries + dock panels |
| **M1** | Diagnostics / leniency | **Mostly** | Panel + CLI + FFI |
| **M1** | Accessible chrome | **Mostly** | QAccessible + focus/keyboard |
| **M1** | Encrypted open | **Mostly** | Password prompt + env password to worker |
| **M1** | Formal a11y audit | **Partial** | `tools/a11y_audit.py` CI + `docs/a11y-audit-checklist.md` manual |
| **M1** | Large-doc p95 budgets | **Harness + gate table** | `tools/bench/p95_gates.toml` + check script; hard gate on ref hardware |
| **M2** | Canonical text model + cache | **Met** | text-extract + coordinator + FFI cache |
| **M2** | Find + geometry | **Mostly** | UTF-8-safe find, CLI/GUI find, selection overlay |
| **M2** | Copy text | **Mostly** | Ctrl+C page text |
| **M2** | Extraction correctness suite | **Mostly** | `text-extract` extraction_correctness (ligature/soft-hyphen/CJK/RTL/unreliable); multi-engine corpus still open |
| **M3** | Fault-injection gate | **Met** | `fault_injection.rs` 7 tests |
| **M3** | Incremental save + recovery | **Met** | coordinator save + sidecar |
| **M4** | Appearance streams always written | **Met** | `build_annotation_pdf_objects` + tests |
| **M4** | XFDF import/export | **Mostly** | Unit interop + shell export/save XFDF |
| **M4** | Annot persist to PDF | **Mostly** | Incremental save + page `/Annots` patch via FFI |
| **M4** | Acrobat/Foxit matrix | **Unit matrix CI** | `interop_matrix` + CI; external apps still release-train |
| **M5** | Forms JS subset | **Subset + Z1 wire + AP regen** | `forms_js` + `CMD:FORMS_CALC`; widget `/AP` via `regenerate_appearances`; shell Forms panel + FFI fill/calc |
| **M5** | AcroForm product fill | **Mostly** | COS leaf-field import on open + session fill/calc/AP/save; seed demo fallback; nested Kids; compressed xref / full FDF flatten still open |
| **M6** | Merge/split/optimize CLI | **qpdf-backed** | `assembly_ops` + CLI; needs `qpdf` on PATH; pure-Rust assembly deferred |
| **M5+** | Full forms/assembly/redaction product exits | **Partial models** | Broader product exit criteria still open |

## Roadmap invariants (SDS §14)

1. No M4+ *shipping* without M3 fault gate — **satisfied**.  
2. Corpus/interop/bench updates with milestones — **in progress**.  
3. GUI/CLI parity where meaningful — **CLI ahead on structure/find; GUI catching up**.  
4. Releasable builds per milestone — **M0 yes; M1–M4 approaching**.
