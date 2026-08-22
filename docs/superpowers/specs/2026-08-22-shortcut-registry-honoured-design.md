# Design: the shell reads its shortcuts from `ui-registry.toml`

**Date:** 2026-08-22
**Milestone:** M1 (ADR-032 stability contract; blocks the M1 "shortcut/menu registry" row)
**Status:** Design only — no code in this change. IG §2.3 requires review before implementation.
**Cites:** ADR-026, ADR-028 §1–§3, ADR-030, ADR-032, ADR-029, DS §14, DS-CONV-4, PRIN-4,
RQA-1..4, UX-KEY-1, GR-8, T-8

---

## Problem

ADR-032 is Accepted and normative: *"No shortcut binding may appear in C++ source. The
registry is the single source of truth; the shell reads it at startup and constructs
`QShortcut`/`QAction` objects from it."* DS-CONV-4 makes that file the concrete expression
of PRIN-4, and ADR-030 makes `profile_version` the identity for policy pinning.

Nothing implements it. `tools/check-ui-registry` (added 2026-07-27) reports **14
violations**: every shortcut is a literal in `shell/canvas/canvas.cc`, and
`shell/chrome/ui-registry.toml` is read by no C++ code — `shell/chrome/` contains no `.cc`
at all. The registry declares 12 actions; the C++ binds 13 keys that are not in it.

Two consequences, neither cosmetic:

1. The stability contract is unenforceable. RQA-1..4 and DS §14 say shortcuts do not change
   except through an approved, versioned change; today a shortcut changes when someone edits
   a `switch` arm, and `profile_version` need never move.
2. The gate is permanently red, so it cannot protect anything. A gate that has never been
   green is indistinguishable from a gate that is broken.

`third_party/MANIFEST.md` also records `toml11 4.x` as "used by shell/chrome (C++)". It is
not vendored — `third_party/` contains only `MANIFEST.md` and `pdfium/`. That row is a claim
about code that does not exist, which is the same defect class the honesty audit removed
elsewhere.

## Goal

The registry is the only place a shortcut is written down. Rebinding an action is a one-line
diff in `ui-registry.toml` plus a `profile_version` bump, with no C++ change — and a test
proves that, rather than asserting it.

## Scope

### In

| Item | Detail |
|---|---|
| Vendor `toml11` | `single_include/toml.hpp` from **v4.4.0**, MIT, 595 KB, **committed** to `third_party/toml11/` with a `provenance.toml` recording ref, URL and SHA-256 |
| `shell/chrome/registry.{h,cc}` | Parses `ui-registry.toml` once at startup: `schema_version` check, `profile_version`, `action -> QKeySequence`, menu taxonomy. New CMake target `shell-chrome` |
| Registry completeness | Every action the shell dispatches must resolve in the registry; a missing action is a startup error, not a silent no-binding |
| `canvas.cc` migration | `CanvasWidget::keyPressEvent` and `MainWindow::keyPressEvent` compare against sequences from the registry instead of `Qt::Key_*` literals and `QKeySequence::StandardKey` |
| Registry contents | The 13 unregistered bindings are added: copy, save, undo, redo, export, go-to-page, find-next, activate, delete, the Down/Space/Up page alternates and the `Ctrl++` zoom alternate |
| `profile_version` | `0.1.0` → `0.2.0`. Adding bindings changes the contract (ADR-030) |
| Tests | Registry parse unit tests; **a QTest that rebinds an action in a fixture registry and asserts the widget follows it** — the only test that can distinguish data-driven from hard-coded |
| Gate | `tools/check-ui-registry` reaches 0 violations and is added to the CI shell job |

### Out

| Item | Why |
|---|---|
| Menu construction from the registry | `shell/chrome/` has no menu implementation yet; this change makes the registry authoritative for *shortcuts*. Menus follow when the chrome exists, under the same file |
| Plugin-contributed items | ADR-014 contributes at runtime; explicitly not part of the stability contract |
| Localization of labels | ADR-032 "Future considerations"; unaffected |
| A `profile_version` diff gate in CI | Worth having (ADR-032 names it), but it needs a prior tagged profile to diff against. Separate work |

## Why vendor the single header rather than fetch it

PDFium is fetched by a setup step because it is a 6 MB per-platform **binary** and SDS §13.4
says so. `toml11` is one MIT-licensed **source** header. ADR-028 §3 wants vendoring with a
pinned ref; committing the file is the most hermetic form of that — a fresh clone builds the
shell with no network at all, and the diff of any future bump is reviewable. The recorded
SHA-256 in `third_party/toml11/provenance.toml` lets the pin be verified against upstream.

## Behaviour when the registry is missing or invalid

Fail closed and loudly (GR-8). The shell exits with a diagnostic naming the file and the
fault. It must not fall back to built-in defaults: a fallback would recreate the very
situation this change removes, where bindings live in C++ and nobody notices.

## Testing

Per ADR-022 strata:

- **T-1** `schema_version` above the compiled-against version is rejected; an unknown action
  is rejected; a malformed key sequence is rejected.
- **T-8** A QTest builds a widget against a fixture registry that binds page-down to a
  non-default key, presses that key, and asserts the step signal fires — and presses the old
  default and asserts it does *not*. Hard-coded bindings fail this test.
- **Gate** `tools/check-ui-registry` reports 0 violations, wired into the CI shell job so a
  new C++ literal cannot land.

## Success criteria

- [ ] `tools/check-ui-registry` is green and runs in CI.
- [ ] No `Qt::Key_*` literal or `QKeySequence::StandardKey` decides a binding in `shell/`.
- [ ] Rebinding an action requires no C++ change, proven by the fixture test.
- [ ] `toml11` is vendored with provenance; `third_party/MANIFEST.md` describes reality.
- [ ] `profile_version` is `0.2.0` and the tracker row says what is and is not covered.

## Risks

- **Qt has opinions.** `QKeySequence::matches` handles platform conventions (`Ctrl` ↔ `Cmd`
  on macOS) that literal comparison does not. The registry stores portable Qt sequence
  strings and `QKeySequence` performs the platform mapping, so this is preserved; the fixture
  test runs on all three OSes in CI to prove it.
- **Startup cost.** One 600 KB header compiled into one translation unit, one file parsed
  once. Immaterial against MET-PERF-1, and measured only if that budget is ever claimed.

---

*Design only. No source file is modified by this change.*
