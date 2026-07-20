# ADR-033 — Wasmtime Adoption for Plugin Runtime

**Status:** Accepted
**Date:** 2026-07-18
**Supersedes:** — (new adoption)
**Cites:** ADR-014, ADR-015, ADR-028, SDS §11, M11

---

## Context

M11 requires a WASM plugin runtime implementing the Component Model with WIT-defined interfaces `[ADR-015]`. The WIT world definition and Rust guest SDK are complete (`plugin-sdk/wit/plugin.wit`, `plugin-sdk/src/lib.rs`). The runtime must now be integrated into the `plugin-host` crate.

## Decision

Adopt **wasmtime 28** as the WASM runtime for the plugin system.

### License

Apache-2.0 / MIT dual license. Permissive; compatible with GPLv3 application and LGPL Qt shell. No copyleft obligations imposed on plugin authors or the core.

### Governance

Bytecode Alliance — multi-stakeholder open-source organization (Mozilla, Fastly, Intel, Microsoft, Fermyon). Active development, regular releases, security-track record. The Component Model and WIT are standards-track work led by this organization.

### Security

- Wasmtime is continuously fuzzed (OSS-Fuzz integration).
- Component Model provides typed, sandboxed isolation by construction.
- Fuel/epoch interruption enforces CPU quotas; store-level memory limits enforce RAM quotas.
- No known sandbox escapes in stable releases.

### Exit seam

Per ADR-015 futures: "If Component Model tooling stalls (review annually), the WIT contracts still stand — they'd retarget to generated C-ABI shims without changing plugin-visible semantics." The WIT world definitions are the stable contract; wasmtime is the implementation.

### Dependency tier

Tier 1 (load-bearing) per ADR-028. Requires written adoption note (this document) reviewed like an ADR.

## Consequences

- `wasmtime = "28"` added to `core/plugin-host/Cargo.toml`.
- Compilation time increase for the `plugin-host` crate (mitigated: wasmtime compiles are cached; the plugin-host crate is not on the critical path for most development).
- Binary size increase (mitigated: WASM runtime is only loaded when plugins are active).
- Annual review of wasmtime governance and Component Model maturity per ADR-028 exit-seam policy.

## Alternatives considered

1. **Wasmer:** capable runtime, but Component Model/WIT center of gravity sits with Wasmtime/Bytecode Alliance. Rejected per ADR-015.
2. **WasmEdge:** stronger in server/edge niches, weaker desktop embedding story. Rejected per ADR-015.
3. **Core-WASM-only with hand-rolled C-ABI:** re-invents interface typing. Rejected per ADR-015.
4. **No WASM runtime yet (defer):** blocks M11 exit criteria. Rejected.

---

*This adoption note satisfies ADR-028 Tier-1 dependency requirements.*
