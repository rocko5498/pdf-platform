# Design: PDFium provisioned from `third_party/`, never fetched at runtime

**Date:** 2026-08-02
**Milestone:** M0 (blocks the confinement exit criterion)
**Status:** Design only — no code in this change. IG §2.3 requires review before implementation.
**Cites:** ADR-005 (engine strategy), ADR-028 (dependency policy: vendoring, provenance,
lockfiles), ADR-029 (prebuilt engine artifacts, path-aware CI), ADR-008/ADR-016 (Z1
confinement), SDS §13.4 (vendored engine), SDS §3.1, SDS §12.2, GR-1, GR-8, GR-9,
NFR-PRIV-2, FR-DIAG-1

---

## Problem

`core/engine-pdfium` depends on `pdfium-auto 0.3`, and `pdfium()` binds through
`pdfium_auto::bind_pdfium_silent()`. That function downloads the PDFium shared library
from `https://github.com/bblanchon/pdfium-binaries` **at runtime, on first use, inside
the Z1 worker process**, into a user cache directory (`<cache>/pdf2md/pdfium-7690/`).

Verified in the crate source (`pdfium-auto 0.3`, `src/lib.rs`):

- `resolve_or_download` returns the cache path on a bare `lib_path.exists()` check.
- `extract_library` unpacks straight onto that final path — no temp file, no atomic
  rename, no cross-process lock.
- There is no checksum, hash, or signature verification anywhere in the crate. The
  only occurrence of "verify" in its source is a comment inside a unit test.

Verified on the dev box: `%LOCALAPPDATA%/pdf2md/pdfium-7690/pdfium.dll`, 5,802,496
bytes, written 2026-07-27, while `third_party/pdfium/prebuilt/` still contains only
`.gitkeep` and `core/engine-pdfium/build.rs` is an explicit stub.

### Why this is a guardrail breach, not a preference

| Rule | Breach |
|---|---|
| GR-1 | Z1 has no network access. The worker fetches over HTTPS during startup. |
| GR-9 / NFR-PRIV-2 | Nothing transmits without an explicit user action. Opening a PDF triggers an outbound request to a third-party host on first run. |
| ADR-028 §3 | Engines are vendored with pinned upstream refs and a maintained patch series. Nothing is vendored. |
| ADR-028 §1 | Every third-party addition records provenance in a `third_party/` manifest. No manifest exists. |
| ADR-028 §2 | The engine is Tier 1 and requires a written adoption note. None exists for `pdfium-auto`. |
| ADR-028 §4 | Lockfiles are law. The actual native code loaded into the process is not covered by any lockfile — it is whatever the URL served. |
| SDS §13.4 | Prebuilt engine artifacts are "fetched by a **setup step**". Runtime is not setup time, and Z1 is not the setup context. |

Unverified native code is dlopen'd into the same process that parses untrusted
documents. That is the supply-chain shape ADR-028 was written against.

### What it drags into the dependency graph

Read from `core/Cargo.lock` and the local registry, not assumed:

- `pdfium-auto` is the **only** dependent of `reqwest` in the workspace. No product
  `Cargo.toml` mentions it.
- `reqwest` is in turn the only root of `hyper`, `rustls`, `quinn` (QUIC),
  `webpki-roots`, and `tokio`. Their dependent chains terminate at `reqwest`.
- So a full HTTP client, a TLS implementation, a QUIC stack, a bundled root-certificate
  store, and an **async runtime** are linked into the Z1 worker — the process whose
  guardrail is that it has no network at all (GR-1), in a product whose guardrail is
  that nothing transmits without an explicit user action (GR-9).
- `tokio` in the worker's graph also sits against GR-6, which keeps async runtimes out
  of the core in favour of threads and channels.
- A license sweep of all 376 locked packages found **no AGPL anywhere**, so ADR-028's
  hard rule holds. The single license outside the ADR-028 allowlist is `webpki-roots`
  (CDLA-Permissive-2.0) — which arrives through this same `reqwest` chain and leaves
  with it. (`ittapi`/`ittapi-sys` are `GPL-2.0-only OR BSD-3-Clause` via wasmtime; the
  BSD-3 option keeps them compliant, and a `deny.toml` should elect it explicitly.)

Removing the runtime download is therefore not only a guardrail repair — it deletes the
entire network stack from the linked graph of an offline-first product.

### It also blocks M0 independently

`sandbox::confinement`'s seccomp policy denies `socket`, `connect`, `bind`, `listen`,
`accept`. While confinement is Advisory the download proceeds. The moment enforcement
is promoted (PR #12), every worker loses its engine at startup. M0's exit criterion
cannot be met while engine acquisition depends on a syscall the sandbox must deny.

### And it is the live CI failure

CI runs `cargo test --workspace`, which runs test binaries in parallel; up to eight
worker processes race the same unlocked cache path. A worker that observes the
partially written library through `exists()` binds a truncated file; a worker whose
mapping is rewritten underneath it dies without stderr. Both present to the
coordinator as `transport disconnected`.

**This symptom was already on record.** `docs/milestone-exit-tracker.md` on
`codex/jobs-scheduler` carries an M0/M3 row dated 2026-07-27 — "PDFium parallel-load
flake (Windows) — **Open defect — not fixed**" — naming the same file and line, the
same `LoadLibraryExW source: 32`, the same 5/8-parallel versus 8/8-serial split, and
observing that it is cold-start only and that "CI gets a cold runner every time". It
closes with "Does not reproduce warm, so it was not diagnosed further and no code
changed." That row is on an unmerged branch, so it is invisible from `main`.

Nothing below re-discovers that symptom. What is new here is **why** it happens — the
cold-start correlation is a cache miss, which means a download is in flight — that it is
not Windows-only, and that the cause is a guardrail breach rather than a flaky test.

Evidence, in three parts.

**1. Non-determinism.** Run `30738219376` on `e8913bd` failed on Ubuntu with two
`coordinator/tests/fault_injection.rs` failures; **re-running the identical commit with
no change passed**. Local verification never caught it because every local run used
`--test-threads=1`, which serialises the race away.

**2. The mechanism, observed directly.** Run `30739814518`, Windows job, on a branch
off `main` (which does not carry PR #15's `retry_bind`): four worker processes panicked
inside the same millisecond, and four of eight fault-injection tests failed.

```
thread 'main' panicked at engine-pdfium\src\backend.rs:40:18:
failed to initialize PDFium: Bind {
  path: "C:\\Users\\runneradmin\\AppData\\Local\\pdf2md\\pdfium-7690\\pdfium.dll",
  reason: "LoadLibraryError(LoadLibraryExW { source: 32 })" }
```

`source: 32` is `ERROR_SHARING_VIOLATION`. The cache path already existed, so this is
not a truncated file — it is concurrent `LoadLibraryExW` against a library another
process is writing, which is exactly what unpacking straight onto the final path with
no lock produces. The `.expect()` at `backend.rs:40` turns that into a worker abort,
which the coordinator can only report as `transport disconnected`.

**3. It is not platform-specific.** The same race produced the Ubuntu failure and this
Windows one. Any claim that this is a Linux quirk is wrong.

`retry_bind` (PR #15) gives 5 attempts over 400 ms total, far short of a cold multi-MB
download, so it narrows the window without closing it — and it leaves the `.expect()`
abort in place for when the retries are exhausted.

---

## Goal

The PDFium binary is a **build/setup-time input with recorded provenance**, and the
worker binds it from a local path. No process in any zone downloads it.

1. A setup step fetches the platform artifact for a pinned upstream ref, verifies it
   against a recorded checksum, and installs it under `third_party/pdfium/prebuilt/`.
2. `third_party/pdfium/` carries a provenance manifest (upstream ref, source URL,
   per-platform artifact names and SHA-256, license).
3. `engine-pdfium` binds via `Pdfium::bind_to_library(path)` against that installed
   path only; `pdfium_auto::bind_pdfium_silent` (the downloading entry point) is not
   called from product code.
4. Absence is a diagnostic, not a download and not a panic: the worker reports the
   engine as unavailable and says how to install it (GR-8, FR-DIAG-1).
5. CI runs the setup step as an explicit, cacheable job step before build.

## Scope

### In

| Item | Detail |
|---|---|
| `third_party/pdfium/provenance.toml` | Upstream ref (`chromium/7690` as currently used), source URL template, per-platform archive name + SHA-256, license (BSD-3-Clause) |
| Setup step | Script under `tools/` that reads the manifest, downloads, verifies the hash, extracts to a temp path, then atomically renames into `third_party/pdfium/prebuilt/<platform>/` |
| `engine-pdfium` bind path | Resolution order: `PDFIUM_LIB_PATH` → `third_party/pdfium/prebuilt/<platform>/` → error. Never download |
| Failure behavior | Typed error surfaced as a diagnostic; worker continues without an engine and says so; no `expect()` abort |
| CI | Setup step added to `.github/workflows/ci.yml` before `cargo build`, cached on the manifest hash |
| Adoption note | Tier 1 note for the engine artifact per ADR-028 §2 (health, governance, bus factor, exit seam = `engine-api` traits) |

### Out

| Item | Why |
|---|---|
| Building PDFium from source (GN/depot_tools) | ADR-029 explicitly runs the full engine build only in CI when `third_party/` changes; prebuilt artifacts are the contributor path |
| Maintained patch series | ADR-028 §3 wants one eventually; nothing is patched today, so an empty series is honest. Separate work |
| Removing `pdfium-auto` from `Cargo.toml` | It may stay usable as a developer convenience behind an explicit off-by-default feature, or be dropped entirely. Decide at review |
| `cargo-deny` license gate | ADR-028 §1 requires it and no `deny.toml` exists anywhere in the repo. Separate, pre-existing gap |
| Reproducible-build attestation of the artifact | ADR-028 names this the hardest remaining supply-chain gap; standing work |

## Open questions for review

1. **Artifact hosting.** `bblanchon/pdfium-binaries` is a third-party rebuild, not an
   upstream Google release. ADR-028 wants pinned provenance; a pinned ref plus SHA-256
   satisfies the letter. Whether the project is willing to depend on that publisher at
   all is a human call.
2. **Committed vs. fetched.** SDS §13.4 says fetched by a setup step, so the artifact
   stays out of git. Confirm that `third_party/pdfium/prebuilt/` remains gitignored and
   only the manifest is committed.
3. **`pdfium-auto`'s fate** — dropped, or retained behind an off-by-default developer
   feature that is never enabled in a shipped or CI build.

## Testing

Per ADR-022 strata:

- **T-1** Unit: manifest parses; checksum mismatch is rejected; missing artifact
  produces the typed "engine unavailable" error rather than a panic or a fetch.
- **T-1** Unit: the resolver prefers `PDFIUM_LIB_PATH`, then the vendored path.
- **T-5** The existing fault-injection suite must pass **without** `--test-threads=1`,
  which is the condition that currently fails. That is the acceptance signal.
- CI: three-OS green on a run with default test parallelism.

## Success criteria

- [ ] No product code path can initiate a network request to obtain the engine.
- [ ] `third_party/pdfium/provenance.toml` records ref, URLs, SHA-256, license.
- [ ] Setup step verifies the checksum and installs atomically.
- [ ] Missing engine reports honestly; no `expect()` abort, no silent success.
- [ ] `cargo test --workspace` (default parallelism) green on all three OSes.
- [ ] Tier 1 adoption note written per ADR-028 §2.

## Relationship to open PRs

- **PR #15** (`retry_bind`) treats the symptom. If this design lands, the retry becomes
  dead weight around a local `dlopen` and should be removed with it. Do not merge #15
  as the fix for the Ubuntu failure — it is not one.
- **PR #12** (enforced confinement) cannot be promoted past Advisory until this lands,
  because its seccomp policy denies the syscalls the current engine path needs.

---

*Design only. No source file is modified by this change.*
