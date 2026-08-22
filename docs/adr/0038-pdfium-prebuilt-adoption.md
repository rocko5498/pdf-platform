# ADR-038 — PDFium Prebuilt Artifact Adoption

**Status:** Proposed
**Date:** 2026-08-21
**Supersedes:** — (adoption note for the engine artifact, replacing the runtime download)
**Cites:** ADR-005, ADR-008, ADR-016, ADR-028 §1–§4, ADR-029, SDS §3.1, SDS §13.4, GR-1,
GR-8, GR-9, NFR-PRIV-2, FR-DIAG-1

---

## Context

ADR-005 makes PDFium the rendering engine, so the native library is Tier 1 by ADR-028 §2's
definition and requires a written adoption note. None existed.

Until this change the library was not vendored at all. `engine-pdfium` depended on
`pdfium-auto 0.3`, whose `bind_pdfium_silent()` downloads the shared library from
`github.com/bblanchon/pdfium-binaries` **at first use, inside the Z1 worker process**,
into a user cache directory, with no checksum, no atomic install, and no cross-process
lock. `docs/superpowers/specs/2026-08-02-engine-prebuilt-provisioning-design.md` records
the full analysis and the review that authorised this implementation.

That arrangement broke four rules at once: GR-1 (Z1 has no network), GR-9 and NFR-PRIV-2
(nothing transmits without an explicit user action), ADR-028 §3 (engines are vendored with
pinned refs), and ADR-028 §4 (lockfiles are law — the native code actually loaded was
whatever the URL served). It also blocked M0: `sandbox::confinement`'s seccomp policy
denies `socket`/`connect`, so promoting confinement past Advisory would have removed the
engine from every worker.

## Decision

Adopt the **`bblanchon/pdfium-binaries` prebuilt of PDFium at ref `chromium/7690`** as a
setup-time input recorded in `third_party/pdfium/provenance.toml`, installed by
`tools/provision_engine.py`, and bound from a local path only.

`pdfium-auto` is **removed** from the workspace rather than retained behind a feature flag.
A dependency whose only purpose is to fetch native code over the network has no
off-by-default form that is safe to keep reachable from Z1. Removing it also drops 98
packages from `core/Cargo.lock` — `reqwest`, `hyper`, `rustls`, `quinn`, `webpki-roots`
and `tokio` among them — an HTTP client, a TLS stack, a QUIC stack, a bundled root
certificate store and an async runtime that were linked into the process whose defining
guardrail is that it has no network. `tokio`'s removal also settles GR-6 for this crate.

`pdfium-render 0.8` stays as the safe binding layer; only the acquisition path changed.

### Licence

PDFium is **BSD 3-Clause**; the artifact ships its `LICENSE`, which the provisioning step
installs beside the library. Passes the ADR-028 §1 allowlist and imposes nothing on the
application.

### Health, governance, bus factor

The upstream engine is Chromium's PDF implementation, maintained by Google with the
security response of a browser component — the strongest possible position for a parser
of untrusted input.

The **artifact publisher is the real risk**, and this note does not soften it.
`bblanchon/pdfium-binaries` is a third-party rebuild by a single maintainer, not a Google
release: upstream publishes no binaries. The mitigations are a pinned ref, a recorded
SHA-256 per platform verified on every install, and a provisioning step that installs
nothing on mismatch. What that gives is **integrity, not provenance**: it guarantees the
bytes are the ones reviewed here, not that the publisher built them from the pinned
upstream source. Reproducing the artifact from source is the open gap ADR-028 names as the
hardest supply-chain problem; it is not closed by this note.

### Exit strategy

`engine-api`'s capability traits are the seam (ADR-005). Replacing the engine means a new
`Rasterize`/`Extract` implementation, not a change to the coordinator, the worker
protocol, or the shell. Replacing the *publisher* — a self-built PDFium, or a distro
package — is a manifest edit plus a path: `provision_engine.py` reads platform, archive
name, checksum and in-archive library path from `provenance.toml`, and `PDFIUM_LIB_PATH`
overrides everything for a local build.

## Consequences

- No product code path can obtain the engine over the network. Acquisition is a setup
  step run by a human or by CI, before the build.
- A missing engine is a diagnostic naming the fix, never a panic, never a fetch, and never
  a silent stub (GR-8, FR-DIAG-1).
- The parallel-load flake recorded against M0/M3 loses its cause: nothing writes to the
  library path while workers read it, because the install is atomic and happens before any
  worker starts.
- Confinement can be promoted past Advisory without removing the engine, which unblocks
  the M0 sandbox criterion.
- `third_party/pdfium/prebuilt/` is gitignored per SDS §13.4; only the manifest is
  committed. A fresh clone must run the setup step, and CI does.
