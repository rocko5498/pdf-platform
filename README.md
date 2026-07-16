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
| **M0** Walking skeleton | **Done** | Tile via bridge+IPC+shmem; kill-respawn; CI + corpus-diff; budgets recorded. Sandbox confinement still advisory. |
| **M1** Robust viewer | **Mostly complete** | Multi-page nav, zoom, GPU/software canvas, a11y surface, outline/diagnostics docks, live structure queries, encrypted open with password prompt, large-doc benches exist. Full multi-tile continuous scroll compositor and formal a11y audit still deepen. |
| **M2** Text / search | **Mostly complete** | Canonical text cache, geometry IPC, find/copy, ligature/CJK-safe search, reliability flag surface. Instant-first-hit streaming and full extraction corpus still expand. |
| **M3** Mutation core | **Gate passed** | CoW, journal, incremental save, autosave recovery, fault-injection suite. |
| **M4** Annotations | **Mostly complete** | Types + appearance streams, QuadPoints, session authoring via tools, XFDF export/import + interop unit tests, ink latency smoke. Real Acrobat/Foxit matrix and document-persisted annot save path still deepen. |
| **M5–M12** | In progress / ahead on models | Forms, assembly, redaction models exist; full milestone exits not claimed. |

### M0 baseline measurements (PRD §14)

| Metric | Measured | Budget | Status |
|--------|----------|--------|--------|
| Cold start (spawn + inspect) | ~11 ms | ≤ 1,000 ms | ✅ |
| First page (spawn + render tile) | ~13 ms | ≤ 300 ms | ✅ |
| Cold start to first pixel | ~14 ms | — | ✅ |

## Build

Requires: Rust stable, and [`qpdf`](https://qpdf.sourceforge.io/) on `PATH` for corpus-diff.

```bash
cd core
cargo build -p cli -p worker-main
cargo run -p cli --bin pdf-platform -- path/to/file.pdf
cargo run -p cli --bin pdf-platform -- find path/to/file.pdf "query"
cargo run -p cli --bin pdf-platform -- diagnostics path/to/file.pdf
cargo test --workspace
cargo run -p corpus-diff   # needs qpdf; exit 0 = gate pass
cargo bench -p benchmarks --bench startup
```

The Qt shell requires Qt 6 (Widgets + OpenGLWidgets) and CMake. The Rust core builds and tests independently.

### Shell shortcuts (M1+)

| Shortcut | Action |
|----------|--------|
| Ctrl+O | Open PDF (password prompt if encrypted) |
| Ctrl+F | Find in document |
| Ctrl+C | Copy current page text |
| Ctrl+E | Export session annotations as XFDF |
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
