# ADR-034 — Tantivy Adoption for Cross-Document Index

**Status:** Accepted
**Date:** 2026-07-21
**Supersedes:** — (new adoption)
**Cites:** ADR-019, ADR-028, SDS §2.2.9

---

## Context

ADR-019 (Search and Indexing Strategy) already decided the cross-document index is "opt-in, local, visible... Tantivy-based index over user-designated folders, built by utility jobs (ADR-009), size-budgeted and inspectable/deletable in settings." The backend-agnostic exit seam this note relies on already exists in code: `core/search/src/cross_document.rs`'s `IndexStaging`/`IndexRecord` types stage bounded, path-free, revision-keyed page text ahead of any concrete index backend, and `core/coordinator/src/broker.rs`'s `IndexEnrollmentRegistry` (commit `5bac26e`) already enforces that only explicitly enrolled roots ever reach that staging layer.

This note satisfies ADR-028's Tier-1 requirement — a written adoption note (health, governance, bus factor, exit strategy) reviewed like an ADR — before `tantivy` actually appears in a `Cargo.toml`. It does not re-decide the choice of Tantivy; ADR-019 already made that decision and rejected SQLite FTS specifically because it drags a C dependency into Z0. This note only clears the ADR-028 gate for adding the crate.

## Decision

Adopt **tantivy 0.22** as the cross-document index backend, implemented behind the existing `IndexStaging` seam in `core/search/src/cross_document.rs`.

### License

MIT license. Permissive; compatible with the GPLv3 application and the LGPL Qt shell, and imposes no copyleft obligations. Recorded in `third_party/MANIFEST.md` per ADR-024/028.

### Governance

Originally authored by Paul Masurel; maintained by Quickwit (a company built around Tantivy-derived search infrastructure) with an active outside-Quickwit contributor base. Widely embedded (Quickwit itself, Meilisearch's early architecture, paradedb, several vector-search projects) — a real bus factor beyond one company's roadmap, though Quickwit remains the dominant maintainer of the core crate.

### Security

- Pure Rust, no C/C++ dependency — the exact property ADR-019 chose it *for* over SQLite FTS (which would add a C parser surface to Z0). Consistent with ADR-016's memory-safety policy (Z0 parsing stays safe Rust).
- Index files are local, never network-fetched; no remote-content parsing surface.
- Runs inside the utility job path (ADR-009), not Z0 directly — index *building* happens in the sandboxed utility pool per SDS §2.4; only the built index and query results return to Z0/coordinator.

### Exit seam

`IndexStaging`/`IndexRecord` (`core/search/src/cross_document.rs`) already define the backend-agnostic boundary: staged records are bounded, path-free, and revision-keyed independent of any concrete index technology. A Tantivy-specific module (e.g. `core/search/src/tantivy_backend.rs`) consumes `IndexStaging`'s output; nothing above that seam should reference Tantivy types directly. Per ADR-028 point 5, this seam is the named migration path if Tantivy's governance or maintenance posture changes.

### Dependency tier

Tier 1 (load-bearing) per ADR-028, which names `tantivy` explicitly in its Tier-1 examples. Requires this written adoption note before the dependency is added, and lockfile-exact, reviewed-PR updates thereafter (ADR-028 point 4).

## Consequences

- `tantivy = "0.22"` to be added to `core/search/Cargo.toml` (currently a stub crate with no index-backend dependency).
- Build time increases for the `search` crate; not on the critical path for most development (matches the wasmtime adoption note's precedent for `plugin-host`).
- Index files live under per-user app state (not beside documents — matches ADR-021's sidecar-hygiene precedent for journals), size-budgeted and user-deletable per ADR-019 point 3 and SDS §2.2.11 settings.
- Annual review of Tantivy/Quickwit governance per ADR-028's exit-seam policy.

## Alternatives considered

Already adjudicated by ADR-019, restated here for the record:

1. **SQLite FTS:** adequate for small corpora, weaker ranking/scaling, and a C dependency in Z0. Rejected per ADR-019.
2. **Engine-native search calls per feature:** duplicates extraction-pathology handling and diverges results across features. Rejected per ADR-019 (this is a search-quality argument, not a backend-choice one, but it rules out *not* having a unified index backend at all).
3. **No cross-document index (defer):** leaves FR-SRCH/-IDX cross-document search unimplemented indefinitely. Rejected — the staging seam is already built and tested; the backend is the remaining gap.

---

*This adoption note satisfies ADR-028 Tier-1 dependency requirements.*
