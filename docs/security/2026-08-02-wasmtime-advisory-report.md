# Security report: 16 RUSTSEC advisories against the pinned Wasmtime

**Date:** 2026-08-02
**Reporter:** agent (drafted; human-gated per IG AI-6, AGENTS §7, CR-Z)
**Status:** Report only. No dependency version was changed.
**Cites:** ADR-014, ADR-015, ADR-028 §2/§4, ADR-033, ADR-029 §3, GR-1, GR-8, FR-PLUG-4, DEP-3, M11

---

## Summary

`cargo deny check advisories` — run for the first time, because nothing in this
repository has ever run one — reports **18 errors against `core/Cargo.lock`**:
16 vulnerabilities and 2 unmaintained crates.

**All 16 vulnerabilities are in `wasmtime 28.0.1`**, the crate ADR-033 adopts as the
plugin sandbox. Several are sandbox escapes.

**They are not exploitable today**, for a reason that is itself a known gap: no
untrusted WebAssembly ever reaches the runtime. That changes the moment M11 does what
it says it does.

## Why they are not currently reachable

- `PluginManager::load` does **not** compile the plugin's bytes. It compiles a
  hardcoded stub — `plugin-host/src/manager.rs:150`:
  ```
  let stub_wat = r#"(module (func (export "_start") (nop)))"#;
  ```
- `PluginRuntime::compile`, the one function that would accept arbitrary bytes
  (`plugin-host/src/runtime.rs:75`), has **no production callers**. Its only callers are
  two unit tests at `runtime.rs:195` and `:202`.
- `PluginRuntime::new` uses `Engine::default()`, so the Cranelift backend is active and
  Winch is not.

So the attack surface these advisories describe — a hostile guest module — does not
exist yet. This matches the already-recorded gap that plugin WASM loading is a stub.

**This is a reprieve, not a mitigation.** Implementing real plugin loading without
first resolving these turns each one live on the same day, in the component whose whole
purpose is containment (`FR-PLUG-4`, `GR-1`).

## The advisories

Locked version: `wasmtime 28.0.1`. Note that **no fix exists on the 28 line** — every
advisory below is fixed in the 24.0.x, 36.x, 40.x, 42.x or 43.x series, so remediation
means moving off 28, not patching within it.

### Would apply with the default Cranelift backend

| ID | Title |
|---|---|
| RUSTSEC-2026-0096 | Miscompiled guest heap access enables **sandbox escape** on aarch64 Cranelift |
| RUSTSEC-2026-0087 | Segfault or unused out-of-sandbox load with `f64x2.splat` on Cranelift x86-64 |
| RUSTSEC-2026-0088 | Data leakage between pooling allocator instances |
| RUSTSEC-2025-0118 | Unsound API access to a WebAssembly shared linear memory |
| RUSTSEC-2026-0091 | Out-of-bounds write or crash transcoding component-model strings |
| RUSTSEC-2026-0093 | Heap OOB read in component-model UTF-16 to latin1+utf16 transcoding |
| RUSTSEC-2026-0092 | Panic transcoding misaligned component-model UTF-16 strings |
| RUSTSEC-2026-0085 | Panic lifting a `flags` component value |
| RUSTSEC-2025-0046 | Host panic via the WASIp1 `fd_renumber` function |
| RUSTSEC-2026-0020 | Guest-controlled resource exhaustion in WASI implementations |
| RUSTSEC-2026-0021 | Panic adding excessive fields to a `wasi:http/types.fields` instance |
| RUSTSEC-2026-0222 | Stores can mix up type indices between engines |

The component-model string and `flags` entries matter most for this project
specifically: ADR-015 puts the plugin contract on the Component Model and WIT, so those
code paths are the ones a real plugin would exercise on every call.

### Winch-backend only — not applicable while `Engine::default()` is used

RUSTSEC-2026-0086 (host data leakage with 64-bit tables), RUSTSEC-2026-0089 (host panic
on `table.fill`), RUSTSEC-2026-0094 (improperly masked `table.grow` return), and
RUSTSEC-2026-0095 (sandbox-escaping memory access). Listed so that anyone enabling Winch knows what
they are turning on.

### Unmaintained

| ID | Crate | Reached via | Solution offered |
|---|---|---|---|
| RUSTSEC-2024-0436 | `paste` | `wasmtime` | none |
| RUSTSEC-2025-0057 | `fxhash` | `fxprof-processed-profile` (wasmtime profiling) | none |

Both leave the tree with a Wasmtime upgrade; neither is separately actionable.

## Why this was invisible

ADR-028 §2 requires a cargo-vet/audit trail for Tier 2 dependencies and §4 makes
lockfiles law with updates landing as reviewed PRs. Nothing enforced either: there is no
`deny.toml` in the repository (a licence-only one is proposed separately), no advisory
check in `.github/workflows/ci.yml`, and no record of `cargo audit` ever being run. A
16-vulnerability window in the sandbox crate stayed open with no signal of any kind.

## What an agent must not do here

Bumping Wasmtime from 28 to a fixed series crosses **fifteen** major versions, in a
security-critical path. IG AI-6 and AGENTS §7 make sandbox and plugin-runtime code
human-gated: an agent may draft, never weaken a verification or relax a sandbox
constraint, and CR-Z requires two reviewers. Attempting the upgrade blind would also
breach ADR-028 §4, which requires dependency updates to land as reviewed proposals
rather than silently.

So this change reports and stops.

## Recommended sequence, for a human to accept or reject

1. **Do not implement real plugin loading before this is resolved.** Right now the stub
   is the only thing standing between these advisories and a hostile plugin.
2. **Decide the target Wasmtime series.** Intersecting every advisory's fixed ranges:

   - **36.0.13 is the smallest version that clears all sixteen**, including the
     Winch-only ones. It is the only viable minimal hop.
   - The 24.0.x line **cannot** work at any patch level: RUSTSEC-2026-0088 and
     RUSTSEC-2026-0096 publish no fix on it.
   - Staying anywhere in 28.x is impossible; nothing is fixed on that line.
   - The modern alternative is **≥47.0.3** (or 46.0.2–46.x), driven by
     RUSTSEC-2026-0222, which needs ≥46.0.2 outside the 24 and 36 lines.

   ADR-033 pins 28 and needs amending either way (ADRU-2), with Component Model API
   churn assessed against `plugin-sdk/wit/`.
3. **Add an advisories gate to CI** once the tree is clean, so the next window is
   measured in days. Deliberately excluded from the licence-gate change so that a
   licence gate cannot fail for reasons that are not licences.
4. **Record the outcome in `docs/adr/0033-wasmtime-adoption.md`**, whose "Security"
   section currently reads "No known sandbox escapes in stable releases" — accurate when
   written, not accurate for the pinned version today.

## Reproducing

```
cd core
cargo deny check advisories
```

Requires `cargo-deny` (verified with 0.20.2) and network access for the advisory
database. Output on 2026-08-02: `advisories FAILED: 18 errors, 0 warnings, 0 notes`.

---

*Report only. No source file, manifest, or lockfile is modified by this change.*
