# ADR-037 — tracing Adoption for Structured Diagnostics

**Status:** Proposed
**Date:** 2026-08-02
**Supersedes:** — (retroactive adoption note for a dependency already in use)
**Cites:** ADR-020, ADR-025, ADR-028 §2, SDS §11, GR-8, GR-9, FR-DIAG-1, NFR-PRIV-2

---

## Context

ADR-028 §2 names `tracing` in Tier 1 and requires a written adoption note covering health,
governance, bus factor, and exit strategy. None existed. The dependency has been in
`core/diagnostics/Cargo.toml` since that crate was created.

## Decision

Adopt **`tracing` 0.1.x** (locked at 0.1.44) with **`tracing-subscriber` 0.3.x** (0.3.23)
as the structured-diagnostics facade, consumed only through the `diagnostics` crate.

### Licence

`MIT` for `tracing`, `tracing-core`, and `tracing-subscriber`. Permissive; passes the
ADR-028 §1 allowlist and imposes nothing on the GPLv3 application.

### Governance and bus factor

Tokio Contributors, under the `tokio-rs` organisation — a multi-maintainer project with
organisational backing, which is a materially better bus-factor position than a
single-author crate. `tracing` 0.1 has been API-stable for years and is the de facto
standard diagnostics facade in the Rust ecosystem.

### Exit seam

The `diagnostics` crate is the seam, and it is a small one: 39 lines across three files.
`init.rs` installs a subscriber from `RUST_LOG`; `redact.rs` provides `Redacted<T>`.

**Verified 2026-08-02:** `tracing::` appears in **no crate outside `diagnostics`**.

That is worth stating plainly rather than dressing up as a clean architecture result:
**the workspace currently emits no trace events at all.** `diagnostics` installs a
subscriber that nothing feeds. The exit seam is intact because the dependency is barely
used, not because it is carefully wrapped. Anyone planning to instrument the core should
decide first whether `tracing`'s macros are used directly across crates — which would
dissolve the seam — or whether `diagnostics` grows a wrapping API that preserves it. That
decision has not been made and this note does not make it.

### Privacy — the reason the seam matters here

ADR-020 and NFR-PRIV-2 forbid document content and file paths reaching logs in release
builds, and `Redacted<T>` exists to enforce that at the type level. If crates begin calling
`tracing::info!` directly with raw values, `Redacted<T>` is bypassed silently and nothing
catches it. Any instrumentation plan needs a matching enforcement story — a lint, a
wrapper API, or a review rule — or ADR-020 becomes advisory in practice.

`tracing` is a local facade only: it writes where a subscriber sends it, and no subscriber
in this tree transmits anything. GR-9 and the no-telemetry guarantee are unaffected, and
must stay that way — a network-shipping subscriber would be a GR-9 breach regardless of how
it were configured.

### Health

Actively maintained, widely deployed, 0.1 API stable. The 0.1 version number reflects
`tracing`'s long-standing pre-1.0 numbering, not immaturity.

### Dependency tier

Tier 1 (load-bearing) per ADR-028 §2.

## Consequences

- `tracing` and `tracing-subscriber` stay in `core/diagnostics/Cargo.toml` and nowhere else.
- A crate adding a direct `tracing` dependency dissolves the seam and should be rejected in
  review until the instrumentation decision above is taken.
- `diagnostics` is not a diagnostics *surface*: FR-DIAG-1's user-visible leniency and
  deviation reporting is separate machinery, and nothing in this note claims otherwise.

## Alternatives considered

1. **`log` + a backend.** Simpler, but no structured fields or spans, which is what SDS §11
   asks for.
2. **Hand-rolled logging.** Rejected under DEP-5: a proven facade is not worth
   reimplementing, and this one is permissively licensed.
3. **No diagnostics facade until instrumentation is actually needed.** Defensible given
   that nothing emits events today, and it is the honest alternative to record. Rejected
   only because `Redacted<T>` and the subscriber wiring are already in place and removing
   them would cost more than keeping them.

---

*Proposed. Awaiting human ratification. Satisfies the ADR-028 §2 Tier 1 requirement for
`tracing` once accepted.*
