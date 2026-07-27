# ADR-035 — UI Action-ID Scope in the Shortcut Registry

**Status:** Proposed — requires human ratification before the gate enforces it
**Date:** 2026-07-27
**Amends:** ADR-032 (schema rules, third bullet)
**Cites:** ADR-032, ADR-030, ADR-004, ADR-025, DS-CONV-4, DS-MENU-2, PRIN-4, ADRU-2, ADRU-6

---

## Context

ADR-032 fixed the format of `shell/chrome/ui-registry.toml` and stated four
normative schema rules. The third reads:

> Every action-id in `[shortcuts]` and in `items[].action` MUST correspond to a
> Command name defined in the `protocol` crate. A CI check enforces this at
> build time.

That check was never built. Writing it revealed the rule is not satisfiable by
any conforming registry.

The shipped registry declares twelve action-ids: `document.open`,
`document.close`, `document.find`, `app.quit`, `view.zoom_in`, `view.zoom_out`,
`view.zoom_fit`, `nav.next_page`, `nav.prev_page`, `nav.first_page`,
`nav.last_page`, `focus.canvas`. The `protocol` crate's `Command` enum contains
`RenderTile`, `Inspect`, `ExtractPage`, `GetOutline`, `GetLayers`,
`GetAttachments`, `GetObject`, `DeletePages`, `RotatePages`, `AddAnnotation`,
`DeleteAnnotation`, `FormsCalc`, `RenderPageForOcr`, `RedactByTerm`, `Quit`,
`LoadPlugin`, `UnloadPlugin`, `InvokePluginAction`.

Zero of the twelve correspond to a Command name. This is not registry drift —
the two vocabularies describe different things and always did. Per ADR-004 the
protocol is a **worker-directed document command/event contract**; per ADR-025
nothing below `protocol` may reference Qt concepts. A UI action such as
`view.zoom_in` or `focus.canvas` is resolved entirely inside the shell: it moves
a viewport or a focus ring and never crosses the bridge. Requiring it to name a
protocol Command would force shell-local view state into the document protocol,
which ADR-003 and ADR-026 exist to prevent.

Enforcing ADR-032's rule as written would therefore reject a correct registry,
and the only way to satisfy it would be to violate ADR-003/004/025/026.

## Decision

Amend ADR-032's third schema rule to the following:

1. An **action-id** is a shell-local identifier in `domain.verb` form. The
   domain prefix is one of a closed set declared in the registry itself, so a
   typo in a prefix is a parse error rather than a silently new namespace.
2. Every action-id used in `items[].action` MUST be declared in `[shortcuts]`.
   The registry is closed under its own references.
3. No two shortcut entries may bind the same key.
4. An action-id MAY declare that it dispatches to a protocol Command, via an
   optional `command` field. **When present, that field MUST name a Command
   variant in the `protocol` crate,** and CI enforces exactly that subset. This
   preserves the enforcement ADR-032 intended, applied to the entries where it
   is meaningful.
5. The rule ADR-032 stated — that *every* action-id names a Command — is
   withdrawn.

ADR-032's other schema rules (integer `schema_version` with parser rejection of
newer files, semver `profile_version` as the ADR-030 profile identity, and "no
shortcut binding may appear in C++ source") are unchanged and remain in force.

## Consequences

- The CI gate becomes implementable. `tools/check-ui-registry` enforces rules
  1–4 today; rule 4's Command cross-check activates once any entry carries a
  `command` field.
- The registry stays the single source of truth for the stability contract
  (DS-CONV-4, PRIN-4), and `profile_version` remains the ADR-030 pinning
  identity. This amendment changes no shortcut, no menu location, and no
  user-visible behavior, so it is **not** a profile-version bump under
  DS §14.2 — it is a correction to a validation rule.
- Per ADRU-6, no PRD/SDS/DS reconciliation is required: the amendment touches
  only how the registry is validated, not what the product does. DS-MENU-2's
  "one canonical menu home per command" is unaffected.
- ADR-032 remains Accepted and is amended, not superseded; this ADR links back
  and must be read alongside it.

## Alternatives considered

**(a) Rename every action-id to a protocol Command name.** Satisfies ADR-032
literally and destroys the registry: `view.zoom_in` and `focus.canvas` have no
Command to name, so they would have to be deleted from the contract or new
Commands invented for shell-local view state — violating ADR-003/004/025/026.
Rejected.

**(b) Add protocol Commands for every UI action.** Inflates the worker-directed
protocol with UI concerns, exactly the boundary erosion ADR-004 was written to
stop ("the shape is a command/event protocol, not an object model"). Rejected.

**(c) Drop the cross-check entirely.** Loses the real enforcement ADR-032
wanted — catching a registry entry that dispatches to a Command that no longer
exists. Rule 4 keeps that value where it applies. Rejected.

**(d) Leave ADR-032 unamended and ship a gate implementing a different rule.**
Illegitimate under ADRU-2: an ADR is superseded or amended, never silently
reinterpreted by the code that claims to enforce it. Rejected — this ADR exists
because that option is not available.

---

*Proposed. The `tools/check-ui-registry` gate currently enforces only the rules
ADR-032 already states unambiguously and explicitly does not implement the
withdrawn rule. It must not be wired into a blocking CI stage until this ADR is
Accepted and the twelve existing C++-bound shortcuts are reconciled.*
