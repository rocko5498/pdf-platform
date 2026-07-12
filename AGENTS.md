# AGENTS.md — Universal AI Agent Coordination

This file governs every AI agent working on this project, regardless of which tool,
CLI, or editor you are running in. Read it fully before touching any code.

---

## 1. What this project is

Open-source, native, cross-platform (Windows/macOS/Linux) professional PDF platform.
Rust core + Qt Widgets shell. Offline-first. No accounts. No telemetry. GPLv3.
Ten-year horizon. Correctness and trust are contractual, not aspirational.

---

## 2. Canonical documents — read before acting

Precedence on conflict: **ADR → SDS → PRD → UI/UX Design System → IMPLEMENTATION_GUIDE**

| File | Authoritative for |
|---|---|
| `docs/adr-constitution.md` | Binding architectural decisions and their rationale |
| `docs/system-design-specification.md` | Component architecture, zones, data flow, lifecycles |
| `docs/product-requirements-document.md` | Product behavior, FR-*/NFR-*/MET-* requirements |
| `docs/ui-ux-design-system.md` | Look, behavior, tokens, DS-* rules |
| `IMPLEMENTATION_GUIDE.md` | Engineering workflow + AI coding rules (§13 binds every agent) |

**Rule:** Specs are ground truth. Do not invent behavior from training data. If the spec
is silent or ambiguous, **stop and ask** — do not guess.

---

## 3. Session start ritual (all agents)

Run this at the start of every work session, in order:

1. **Load context** — read `AGENTS.md` (this file) and `CLAUDE.md` if present.
2. **Check current milestone** — read `docs/system-design-specification.md §14`
   to know which milestone (M0–M12) is active.
3. **Check open work** — read `.agent-state/handoff.md` if it exists (§6 below).
4. **Declare intent** — before editing, write a one-line entry to `.agent-state/log.md`:
   `[ISO-timestamp] [agent-id] starting: <what you plan to do>`
5. **Confirm scope** — if the user has not stated what to build, ask before proceeding.

---

## 4. Hard rules — never violate

These map to `IMPLEMENTATION_GUIDE §3` and `ADR-016`. A change violating any of these
is rejected regardless of feature value.

| Rule | Constraint |
|---|---|
| GR-1 | Respect trust zones Z0/Z1/Z2/Z3. No document parsing in Z0. No Z1 network access. |
| GR-2 | Single writer. Mutate only via Commands through the coordinator. No back-door writes. |
| GR-3 | One FFI boundary. All Rust↔Qt traffic through the `bridge` crate only. |
| GR-4 | All engine calls through the capability trait. No direct PDFium calls outside the seam. |
| GR-5 | Non-destructive by default. Saving is incremental; untouched bytes are never rewritten. |
| GR-6 | No async runtime in the core. Threads + channels only. Core API is not function-colored. |
| GR-7 | Every cache/container growing with document size declares a bound/eviction policy. |
| GR-8 | Honesty over silent failure. Tolerated deviations surface via diagnostics, never false success. |
| GR-9 | No default network or telemetry. Nothing transmits without an explicit user action. |
| GR-10 | No hard-coded UI values. All user-visible code uses design tokens. |

---

## 5. What to cite in every change

Every non-trivial change **must** reference the IDs it implements. Include them in commit
message body and PR description. A change with no citation is out of scope until traced.

- Functional requirements: `FR-<area>-<n>` (from PRD §9)
- Non-functional: `NFR-<area>-<n>`
- Architecture decisions: `ADR-NNN`
- SDS sections: `SDS §N.N`
- Design-system rules: `DS-<area>-<n>`

---

## 6. Distributed work coordination

When multiple agents (or humans and agents) work in parallel, use `.agent-state/` to
coordinate. This directory is **not committed** (gitignored); it is ephemeral session state.

### `.agent-state/handoff.md` — structured handoff

Format (append-only, newest at top):

```markdown
## [ISO-timestamp] — [agent-id or "human"]

**Milestone:** M0 (or whichever is active)
**Branch/PR:** main / PR#N (if applicable)
**Done:**
- Brief bullet of what was completed, with citation IDs
**In-flight:**
- Work started but not finished (file paths, what remains)
**Blocked:**
- Anything waiting on human decision or another agent
**Next suggested:**
- Concrete next task with citation IDs
**Files touched:**
- List of modified files
```

### `.agent-state/log.md` — append-only activity log

One line per action:
```
[ISO-timestamp] [agent-id] <verb>: <what> (<file:optional>)
```

### Claiming work

Before starting a task that another agent might also pick up, append to `.agent-state/claims.md`:
```
[ISO-timestamp] [agent-id] CLAIM: <task description> (citation: FR/ADR/SDS IDs)
```

Check existing claims before starting. If a claim is stale (> 2 hours, no log activity),
it is considered abandoned and may be reclaimed.

---

## 7. Security-critical paths — human-gated

An agent **may draft** code for these areas but **must not** weaken a verification,
downgrade an indeterminate result, or relax a sandbox constraint. Mark drafts clearly.
A human owner must review before merge.

- Redaction (`FR-RED-*`, `SDS §3.3.1`)
- Digital signatures (`FR-SIG-*`, `SDS §2.8`)
- Sandbox confinement (`ADR-008`, `ADR-016`, `SDS §12.2`)
- Crypto (`ADR-028`)
- FFI/bridge (`ADR-004`, `ADR-027`)

---

## 8. Dependency policy

- No new dependency without flagging it (license + exit seam analysis). AGPL forbidden in linked code.
- No `unsafe` without `// SAFETY:` proof comment and a request for second review.
- Pin all versions. No floating majors.

---

## 9. Tests are part of the change

Do not mark work done without the required test strata (`IMPLEMENTATION_GUIDE §8`):
unit/property, corpus regression, differential, fuzzing (for untrusted-input paths),
fault injection (for mutation-core paths), conformance/interop.

---

## 10. Session close

When finishing a work session (regardless of whether the task is complete):

1. Write a handoff entry to `.agent-state/handoff.md` (§6 format).
2. Log a closing entry to `.agent-state/log.md`.
3. If a claim was made, either close it (mark DONE) or leave it active with a status note.
4. Ensure the working tree is in a known state: either committed, or all in-progress files
   described in the handoff.

---

## 11. Milestone reference (current target: M0)

| Milestone | Goal | Exit criteria |
|---|---|---|
| **M0** | Walking skeleton | Tile rendered through real bridge+IPC+shmem on all 3 OSes; worker sandboxed; kill-worker respawn verified; CI + corpus-diff harness exist |
| M1 | Robust viewer | Smooth scroll, large-doc benchmarks, repair/leniency, accessible chrome |
| M2 | Text/search | Extraction correctness, find latency, reliability flagging |
| M3 | Mutation core | Fault-injection suite; incremental save; crash recovery — **gates all editing features** |
| M4–M12 | Features | See `SDS §14` |

**Do not build editing features (M4+) before M3 passes its fault-injection gate.**

---

## 12. Quick reference — workspace layout

```
core/          Rust workspace (all domain logic)
  pdf-cos      COS object store, xref, filters
  pdf-types    Shared primitives
  pdf-model    Semantic façades, Commands, journal
  pdf-write    Serializers (incremental + rewrite)
  engine-api   Capability traits (no engine knowledge)
  engine-pdfium  PDFium backend
  coordinator  Wires everything; the trusted brain
  worker-main  Z1 binary (sandboxed)
  protocol     Command/event types (shared contract)
  ffi-bridge   Single cxx boundary
  cli          Headless client
shell/         Qt 6 Widgets (C++); thin, stateless
plugin-sdk/    WIT worlds + bindings
docs/          Canonical specs (read-only for agents)
tools/         Corpus-diff, benchmark harness
third_party/   Vendored engines (PDFium etc.)
```

---

*This file is tool-agnostic and applies equally to Claude Code, Cursor, Copilot,
Aider, or any other AI coding agent. Keep it up to date as the project evolves.*
