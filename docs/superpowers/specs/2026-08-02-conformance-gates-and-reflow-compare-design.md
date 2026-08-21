# Design (retroactive): two conformance gates and reflow-resilient compare

**Date:** 2026-08-02
**Milestone:** cross-cutting — supports M0 governance and M12 compare
**Status:** Retroactive. The code exists on `codex/jobs-scheduler` (PR #10) and shipped
without the note IG §2.3 requires. This pays that debt so the three additions are
reviewable as designs, not only as diffs.
**Cites:** IG §2.3, IG §13 (G-1, AI-2, AI-9), ADR-030, ADR-032, ADR-022, DS-CONV-4,
DS-MENU-3, DS-PHIL-3, PRIN-4, RQA-1, RQA-2, FR-CMP-1, FR-CMP-3, GR-7, GR-8

---

## Why one note and not three

G-2 forbids duplicating canonical content. Each module already carries a header that
states its rules and cites them; restating that three times would be the duplication
G-2 rules out. What was actually missing is the reviewable record of **scope, bounds,
and the decisions each module leaves open** — that is what this note supplies.

The alternative the prior handoff offered was dropping the additions. That is worse:
two of the three enforce rules the constitution already binds contributors to, and the
third fixes a requirement violation.

---

## 1. `tools/check-ui-registry` — ADR-032 conformance gate

**Purpose.** `shell/chrome/ui-registry.toml` is the interface-stability contract as an
artifact [ADR-032, ADR-030, DS-CONV-4, PRIN-4]. Nothing verified the artifact matched
the contract, and nothing verified the C++ shell honored it.

**Enforced:** `schema_version` is an integer within the parser's support; `profile_version`
is semver; no two actions bind the same key; every menu item is an action or a separator,
its action exists in `[shortcuts]`, and any shortcut it displays is one of that action's
declared keys; no shortcut binding appears in C++ source under `shell/`.

**Deliberately not enforced — this is the decision needing review.** ADR-032 also
requires every action-id to name a Command in the `protocol` crate. The shipped registry
uses shell-local view actions (`view.zoom_in`, `focus.canvas`, `nav.next_page`) that are
not, and cannot be, worker-directed protocol Commands. **Zero of twelve ids satisfy the
rule as written**, so enforcing it would reject the entire registry. The gate does not
silently substitute a weaker rule of its own invention (AI-1); the rule is left
unenforced and named in the module header. Resolving it requires an amending or
superseding ADR (ADRU-2). Reviewer decision needed: amend ADR-032 to scope the rule to
worker-directed actions, or change the registry.

**Standing output.** The gate currently reports 13 real C++-hardcoded shortcut
violations. Clearing them changes the UI profile identity, which is a `profile_version`
bump under ADR-030 — and AI-9 bars an agent from touching the stability contract. That
work is a human's.

**Bounds and dependency.** `toml 0.8`, MIT OR Apache-2.0, already in `core/Cargo.lock`
via existing crates, so no new supply-chain surface (DEP-1). Only this crate parses the
registry, so the parser is swappable in one file (DEP-2 exit seam).

## 2. `tools/check-citations` — G-1 traceability gate

**Purpose.** G-1 requires every non-trivial change to cite the requirement or decision
it implements, and AI-2 declares an uncitable change out of scope. Nothing checked that
a cited identifier *exists*, so a typo (`FR-ANNOT-9`, `ADR-0.3`) read as traceable and
passed CI. A traceability rule that cannot detect a dangling citation enforces nothing.

**Design.** Harvest every identifier defined in the canonical documents
(`adr-constitution`, `system-design-specification`, `product-requirements-document`,
`ui-ux-design-system`, `IMPLEMENTATION_GUIDE`, `AGENTS`), then report citations in
source matching none of them. A citation resolves when it is an exact identifier
(`FR-ANNOT-2`) or a family prefix of one (`FR-ANNOT`, as written in `[FR-ANNOT-*]`),
because citing a whole family is idiomatic throughout this repo.

**Deliberate limit.** The gate proves an identifier *exists*, never that it is the
*right* one. Citing `ADR-005` for a redaction change still passes. Semantic aptness is a
reviewer's job (CR-10) and the gate does not pretend otherwise (GR-8).

## 3. `core/text-extract/src/compare.rs` — reflow-resilient line compare

**Purpose.** FR-CMP-3 requires textual comparison resilient to reflow and pagination,
"prioritizing meaningful change detection over raw positional diff". Pairing lines by
index fails that outright: inserting one line at the top reports every following line as
changed, which is precisely the raw positional diff the requirement excludes.

**Design.** Longest-common-subsequence alignment, so an insertion or deletion costs one
operation instead of shifting everything after it.

**Bound, per GR-7.** The dynamic-programming table is `(before+1) * (after+1)` `u32`
cells, so `MAX_ALIGNED_LINES = 2000` caps it near 16 MB. Beyond the bound the comparison
degrades to positional pairing rather than allocating without limit, and `diff_lines`
reports which path it took via `DiffQuality` — the degraded result is labelled, never
presented as an alignment (GR-8).

**Still open.** FR-CMP-1 visual compare and FR-CMP-2 move detection are unbuilt. This
note does not claim M12 exit.

---

## Testing (ADR-022)

- **T-1** Both gates carry unit tests over fixture inputs, including the negative cases
  (duplicate binding, unknown menu action, dangling citation). `compare` carries unit
  tests for insertion, deletion, and the over-bound fallback path.
- **T-2/T-3/T-5** Not applicable: no rendering, extraction-oracle, or mutation-core path
  is touched.
- Neither gate is wired into `.github/workflows/ci.yml` yet. Until it is, both are
  runnable tools rather than gates, and no claim of enforcement should be made (GR-8).

## Success criteria

- [ ] Reviewer accepts or rejects the ADR-032 unenforced-rule decision above (ADRU-2).
- [ ] Human decides the `profile_version` bump that clearing the 13 violations requires.
- [ ] If the gates are to bind, they are added to CI in a separate change.

---

*Retroactive record. No code changes with this note.*
