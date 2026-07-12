# Project: Open-source professional PDF platform

## Canonical documents (authoritative - read before acting)
Precedence on conflict: ADR -> SDS -> PRD -> UI/UX Design System -> IMPLEMENTATION_GUIDE.
- docs/adr-constitution.md - binding decisions + rationale
- docs/system-design-specification.md - architecture, zones, lifecycles
- docs/product-requirements-document.md - behavior, scope, FR-*/NFR-*/MET-*
- docs/ui-ux-design-system.md - look, behavior, tokens, DS-*
- IMPLEMENTATION_GUIDE.md - workflow + AI coding rules (section 13 binds you)

## Hard rules (from IMPLEMENTATION_GUIDE section 13 - do not violate)
- Specs are ground truth, NOT your training. Don't invent behavior or
  "improve" decisions. If a spec is silent/ambiguous, STOP and ask.
- Cite the IDs you implement (FR-*, ADR-*, SDS section, DS-*) in every change.
- Never cross an architectural guardrail (IG section 3): no second FFI path,
  no engine calls outside the trait, no shared-mutable document state,
  no async runtime in the core, no hard-coded UI values, no default
  network/telemetry.
- No new dependency without flagging it (license + exit-seam).
- No `unsafe` unless explicitly instructed, with a // SAFETY: proof.
- Security-critical code (redaction, signatures, sandbox, crypto, FFI)
  is human-gated: draft only, never weaken a verification or guardrail.
- Tests are part of the change. Don't fabricate benchmarks or results.
- Respect the stability contract: don't change shortcuts, menu taxonomy,
  focus order, or default workspaces.
- Small, reviewable diffs. Surface uncertainty honestly.
