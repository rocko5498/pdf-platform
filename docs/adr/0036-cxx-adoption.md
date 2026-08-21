# ADR-036 — cxx Adoption for the Rust↔C++ Bridge

**Status:** Proposed
**Date:** 2026-08-02
**Supersedes:** — (retroactive adoption note for a dependency already in use)
**Cites:** ADR-003, ADR-004, ADR-027, ADR-028 §2, SDS §4.1, SDS §12.4, GR-3, FFI-1..6

---

## Context

ADR-004 makes the `bridge` crate the single Rust↔Qt language boundary, and FFI-1 requires
that boundary to use "the `cxx`-checked interface only. No hand-rolled ABI, no raw
`extern "C"` shims outside the bridge crate." `cxx` is therefore load-bearing by
constitutional mandate, not by convenience.

ADR-028 §2 names `cxx` explicitly in Tier 1 and requires "a written adoption note —
health, governance, bus factor, exit strategy — reviewed like an ADR". No such note
existed. This is that note, written after the fact; the dependency has been in
`core/ffi-bridge/Cargo.toml` since the bridge was built.

## Decision

Adopt **`cxx` 1.x** (locked at 1.0.197) as the Rust↔C++ interface generator, with
`cxx-build` 1.x as its build-time counterpart.

### Licence

`MIT OR Apache-2.0`. Permissive; imposes nothing on the GPLv3 application or the LGPL Qt
shell. Passes the ADR-028 §1 allowlist.

### Governance and bus factor

**Sole author and maintainer: David Tolnay.** This is the honest bus-factor statement and
the main risk in this note. Mitigating facts: the crate is among the most depended-upon in
the Rust ecosystem, it is API-stable at 1.x with a long release history, and the same
author maintains several other foundational crates already in this tree. None of that
changes the count. There is no organisation behind it and no succession plan published.

### Health

Actively released; 1.x has held API compatibility since 2020. The generated code is
checked at compile time on both sides, which is the property FFI-1 is buying: a mismatch
between the Rust and C++ views of the boundary is a build error rather than undefined
behaviour at runtime.

### Exit seam

`ADR-004` is the seam. All Rust↔Qt traffic crosses one surface as commands, events, and
handle descriptors (`SDS §12.4`), so a replacement generator changes how that surface is
expressed, not what crosses it.

**Verified 2026-08-02:** `cxx::` appears in exactly one file, `core/ffi-bridge/src/lib.rs`
(`#[cxx::bridge]` at line 1204). No other crate in the workspace references it. The seam
is intact in fact, not only on paper.

Replacing it would mean hand-writing the `extern "C"` layer that FFI-1 currently forbids,
or adopting another generator. Either is a bounded, single-crate change — which is the
whole point of DEP-2.

### Security

`cxx` generates the unsafe glue rather than leaving it hand-written, which is the safer
default under ADR-027 (`unsafe` confined to designated reviewed modules) and UNSAFE-5.
It does not itself provide isolation: the bridge runs in Z0, and FFI-5 still requires every
payload to be defined once in the `protocol` crate and validated on receipt.

### Dependency tier

Tier 1 (load-bearing) per ADR-028 §2. FFI-6 additionally requires two reviewers, one owning
the FFI surface, for any change to this boundary.

## Consequences

- `cxx = "1"` and `cxx-build = "1"` stay in `core/ffi-bridge/Cargo.toml`; `build.rs` drives
  generation.
- The bridge is the only crate permitted to depend on either. A second crate taking a
  dependency on `cxx` is a GR-3 violation and should be rejected in review.
- Annual review of maintainer health per the ADR-028 exit-seam policy, with the bus factor
  as the specific thing being watched.

## Alternatives considered

1. **Hand-rolled `extern "C"` shims.** Rejected by FFI-1 before this note existed: no
   compile-time checking of the two-language contract, and it spreads `unsafe` across the
   boundary rather than concentrating it.
2. **`autocxx`.** Builds on `cxx`, so it inherits the same bus factor while adding
   generation complexity the bridge does not need.
3. **A C ABI plus manual bindings on the Qt side.** Portable, but re-creates by hand
   exactly what FFI-2 and FFI-3 want checked.

---

*Proposed. Awaiting human ratification. Satisfies the ADR-028 §2 Tier 1 requirement for
`cxx` once accepted.*
