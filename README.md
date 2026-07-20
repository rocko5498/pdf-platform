# PDF Platform

An open-source, native, desktop-first professional PDF platform — a
trustworthy alternative to Adobe Acrobat Pro. Offline-first, private,
standards-based, built for a 10+ year maintenance horizon.

## Documentation

Read these before contributing. They are authoritative; code follows them.

- **[IMPLEMENTATION_GUIDE.md](IMPLEMENTATION_GUIDE.md)** — how we work
  (workflow, guardrails, AI coding rules). Read this first.
- **[docs/adr-constitution.md](docs/adr-constitution.md)** — binding
  engineering decisions and rationale.
- **[docs/system-design-specification.md](docs/system-design-specification.md)**
  — architecture and system design.
- **[docs/product-requirements-document.md](docs/product-requirements-document.md)**
  — product behavior, scope, requirements.
- **[docs/ui-ux-design-system.md](docs/ui-ux-design-system.md)** —
  look, behavior, tokens.

Precedence on conflict: ADR → SDS → PRD → UI/UX Design System → Implementation Guide.

## Status (roadmap vs SDS §14)

| Milestone | Status | Notes |
|-----------|--------|--------|
| **M0** Walking skeleton | **Done** | Tile via bridge+IPC+shmem; kill-respawn; CI + corpus-diff; budgets recorded. Sandbox confinement still **advisory**. |
| **M1** Robust viewer | **Mostly complete** | Multi-tile continuous scroll, zoom, GPU/software canvas, a11y static audit, live outline/diagnostics, encrypted open, large-doc benches. Hard p95 on ref hardware still open. |
| **M2** Text / search | **Mostly complete** | Text cache, geometry IPC, find/copy, UTF-8-safe ligature/CJK find, `extraction_correctness` suite. Multi-engine corpus still expands. |
| **M3** Mutation core | **Gate passed** | CoW, journal, incremental save, autosave recovery, fault-injection suite. |
| **M4** Annotations | **Mostly complete** | Appearance streams, QuadPoints, tools, XFDF + interop unit matrix, incremental save with `/Annots` patch + XFDF sidecar. External Acrobat/Foxit lab still open. |
| **M5** Forms JS | **Subset + wire + AP + COS import** | `forms_js`, kill switch, `CMD:FORMS_CALC`, widget `/AP`, shell Forms panel, **AcroForm leaf import on open**. Flatten/FDF full product still open. |
| **M6** Assembly | **CLI (qpdf)** | merge / split / extract-pages / optimize + preflight honesty. Pure-Rust assembly deferred. |
| **M7** Redaction | **CLI complete** | `redact-by-term` command with text search, content removal, verification pass, signed report. |
| **M8** Signatures | **CLI complete** | `validate-signatures` command with ByteRange validation, DocMDP analysis, plain-language reporting. |
| **M9** OCR | **CLI + coordinator wired** | `ocr` command with Tesseract backend, text layer generation, coordinator `ocr_page` method, `RenderPageForOcr` protocol. |
| **M10** Compliance | **CLI complete** | `validate-pdf-a` command with level selection (1a–4), errors/warnings reporting. |
| **M11** Plugins | **CLI complete** | `plugin-list` + `plugin-validate` commands, plugin-host with Wasmtime runtime, capability grants, circuit breaker. |
| **M12** Compare | **CLI complete** | `compare` command with text-based document comparison, line-by-line diff. |

Criterion-level status: [docs/milestone-exit-tracker.md](docs/milestone-exit-tracker.md).  
Release checklist: [docs/release-gates.md](docs/release-gates.md).

### M0 baseline measurements (PRD §14)

| Metric | Measured | Budget | Status |
|--------|----------|--------|--------|
| Cold start (spawn + inspect) | ~11 ms | ≤ 1,000 ms | ✅ |
| First page (spawn + render tile) | ~13 ms | ≤ 300 ms | ✅ |
| Cold start to first pixel | ~14 ms | — | ✅ |

## Build

Requires: Rust stable, and [`qpdf`](https://qpdf.sourceforge.io/) on `PATH` for corpus-diff and assembly CLI.

```bash
cd core
cargo build -p cli -p worker-main
cargo run -p cli --bin pdf-platform -- path/to/file.pdf
cargo run -p cli --bin pdf-platform -- find path/to/file.pdf "query"
cargo run -p cli --bin pdf-platform -- export-text path/to/file.pdf 0
cargo run -p cli --bin pdf-platform -- optimize-preflight path/to/file.pdf screen
cargo run -p cli --bin pdf-platform -- merge a.pdf b.pdf -o out.pdf
cargo run -p cli --bin pdf-platform -- split in.pdf -o split-out/
cargo run -p cli --bin pdf-platform -- redact-by-term doc.pdf --term "SECRET"
cargo run -p cli --bin pdf-platform -- validate-signatures doc.pdf
cargo run -p cli --bin pdf-platform -- validate-pdf-a doc.pdf --level 2b
cargo run -p cli --bin pdf-platform -- ocr scanned.pdf --lang eng
cargo run -p cli --bin pdf-platform -- compare doc_v1.pdf doc_v2.pdf
cargo run -p cli --bin pdf-platform -- plugin-list
cargo run -p cli --bin pdf-platform -- plugin-validate plugin.json
cargo run -p cli --bin pdf-platform -- forms-calc-demo
cargo run -p cli --bin pdf-platform -- confinement
cargo test --workspace
cargo run -p corpus-diff   # needs qpdf; exit 0 = gate pass
cargo bench -p benchmarks --bench startup
# from repo root:
python tools/a11y_audit.py
python tools/bench/check_p95_gates.py
```


The Qt shell requires Qt 6 (Widgets + OpenGLWidgets) and CMake. The Rust core builds and tests independently.

### Shell shortcuts (M1+)

| Shortcut | Action |
|----------|--------|
| Ctrl+O | Open PDF (password prompt if encrypted) |
| Ctrl+F | Find in document |
| Ctrl+C | Copy current page text |
| Ctrl+E | Export session annotations as XFDF |
| Ctrl+G | Run forms calculations + regenerate widget appearances |
| Ctrl+S | Save PDF (incremental annots/forms + XFDF sidecar) |
| PageUp/Down, wheel | Previous/next page |
| Ctrl+wheel / Ctrl± | Zoom |
| Enter (with tool) | Place annotation |
| F6 | Focus canvas |

## Architecture (quick reference)

```
core/          Rust workspace
  pdf-cos      COS object store, xref, filters
  pdf-model    Semantic façades, Commands, journal, annotations, XFDF
  pdf-write    Incremental + rewrite serializers
  engine-api   Capability traits
  engine-pdfium  PDFium backend
  coordinator  Trusted brain (Z0)
  worker-main  Z1 binary
  protocol     Command/event contract
  ffi-bridge   Single cxx boundary
  search       Find + normalization
  cli          Headless client
shell/         Qt 6 Widgets + OpenGL canvas
docs/          Canonical specs
tools/         Corpus-diff, benchmarks
third_party/   Vendored engines (PDFium)
```

## License

GPLv3 — see [LICENSE](LICENSE).
