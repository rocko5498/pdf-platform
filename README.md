# PDF Platform

An open-source, native, desktop-first professional PDF platform - a
trustworthy alternative to Adobe Acrobat Pro. Offline-first, private,
standards-based, built for a 10+ year maintenance horizon.

## Documentation

Read these before contributing. They are authoritative; code follows them.

- **[IMPLEMENTATION_GUIDE.md](IMPLEMENTATION_GUIDE.md)** - how we work
  (workflow, guardrails, AI coding rules). Read this first.
- **[docs/adr-constitution.md](docs/adr-constitution.md)** - binding
  engineering decisions and rationale.
- **[docs/system-design-specification.md](docs/system-design-specification.md)**
  - architecture and system design.
- **[docs/product-requirements-document.md](docs/product-requirements-document.md)**
  - product behavior, scope, requirements.
- **[docs/ui-ux-design-system.md](docs/ui-ux-design-system.md)** -
  look, behavior, interaction, tokens.

Precedence on conflict: ADR -> SDS -> PRD -> UI/UX Design System -> Implementation Guide.

## Status

**Milestone M0** (walking skeleton) per SDS §14 — in progress.

| Piece | Status |
|-------|--------|
| CLI structural summary (`pdf-platform <file>`) | Working on `main` / PR stack |
| Corpus-diff harness (our scanner vs `qpdf`) | On PR — must stay green in CI |
| Tile via bridge + IPC + shmem + sandbox | Not yet (hard plumbing next) |
| CI gate | GitHub Actions: build/test + corpus-diff on 3 OSes |

## Build (Rust tools only, for now)

Requires: Rust stable, and [`qpdf`](https://qpdf.sourceforge.io/) on `PATH` for corpus-diff.

```bash
cd core
cargo build -p cli
cargo run -p cli --bin pdf-platform -- path/to/file.pdf
cargo test --workspace
cargo run -p corpus-diff   # needs qpdf; exit 0 = gate pass
```

The Qt shell and sandboxed worker render path are not wired yet — that is the rest of M0.

## License

GPLv3 — see [LICENSE](LICENSE).
