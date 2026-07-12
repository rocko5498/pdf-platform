# ADR-032 — Shortcut and Menu Registry File Format

**Status:** Accepted

**Context.** `[ADR-026]` specifies that `shell/chrome/` contains "a versioned data file" representing the keyboard shortcut map and menu taxonomy, described as "the interface-stability contract (`[ADR-001]` value 3) as an artifact under diff review." `[ADR-030]` establishes that UI profile versions are the unit of the stability contract and that changes to shortcuts or menu taxonomy constitute a profile-version bump. The Design System rule `DS-CONV-4` designates this file as the concrete expression of `PRIN-4`.

The file must exist before any `chrome/` implementation begins — its diff-readability is the enforcement mechanism for the stability contract in code review, and the `profile_version` field is the identity ADR-030 uses for policy pinning. No existing ADR fixes the format, schema structure, or C++ parsing strategy.

**Problem statement.** Choose a file format for the shortcut/menu registry satisfying: (a) human-readable and diff-friendly — a reviewer must be able to identify a changed shortcut or menu item by reading the diff alone; (b) supports a `profile_version` field for ADR-030 UI profile pinning and a `schema_version` field for parser compatibility; (c) supports hierarchical structure (menu trees) and flat key-to-action bindings; (d) admits inline comments for intent documentation (rationale for non-obvious bindings); (e) parseable from C++/Qt without a heavyweight or GPL-licensed parser dependency; (f) the format is the single source of truth — no shortcut may be defined in C++ code.

**Decision.** TOML (Tom's Obvious Minimal Language), with the schema below. The C++ parser is `toml11` v4 (MIT, header-only, no transitive dependencies — qualifies as Tier 2 per `[ADR-028]`).

**Canonical file:** `shell/chrome/ui-registry.toml`

```toml
# Shortcut and menu registry — the interface-stability contract.
# Governed by ADR-032. Any change to a binding or menu item requires
# bumping profile_version and follows the review policy of ADR-030.

schema_version = 1        # integer; increment on breaking schema changes only
profile_version = "1.0"   # semver string; the ADR-030 UI profile identity

# ---------------------------------------------------------------------------
# Keyboard shortcuts
# key:   action-id (string, matches the coordinator's protocol Command name)
# value: Qt key-sequence string (e.g. "Ctrl+O", "Alt+Left")
# ---------------------------------------------------------------------------
[shortcuts]
open            = "Ctrl+O"
save            = "Ctrl+S"
save_as         = "Ctrl+Shift+S"
close_document  = "Ctrl+W"
quit            = "Ctrl+Q"
undo            = "Ctrl+Z"
redo            = "Ctrl+Shift+Z"
find            = "Ctrl+F"
previous_view   = "Alt+Left"    # Acrobat-classic navigation; DS-PHIL-1
next_view       = "Alt+Right"
zoom_in         = "Ctrl++"
zoom_out        = "Ctrl+-"
fit_page        = "Ctrl+0"
fit_width       = "Ctrl+2"
# ... (complete table defined at implementation time)

# ---------------------------------------------------------------------------
# Menu taxonomy
# Each [[menus]] block is one top-level menu in document order.
# item fields: action (string, action-id) | separator (bool) | submenu (string, id of nested [[menus]])
# ---------------------------------------------------------------------------
[[menus]]
id    = "file"
label = "&File"
items = [
  { action = "open",           label = "&Open…"            },
  { separator = true },
  { action = "save",           label = "&Save"             },
  { action = "save_as",        label = "Save &As…"         },
  { separator = true },
  { action = "close_document", label = "&Close"            },
  { separator = true },
  { action = "quit",           label = "&Quit"             },
]

[[menus]]
id    = "edit"
label = "&Edit"
items = [
  { action = "undo",  label = "&Undo" },
  { action = "redo",  label = "&Redo" },
  { separator = true },
  { action = "find",  label = "&Find…" },
]

# ... (complete table defined at implementation time)
```

**Schema rules (normative):**

- `schema_version` is an integer. A parser MUST reject a file whose `schema_version` exceeds the version it was compiled against.
- `profile_version` is a semver string. It is the identity key for ADR-030 profile pinning; it MUST be bumped on any change to `[shortcuts]` bindings or `[[menus]]` item order, labels, or structure.
- Every action-id in `[shortcuts]` and in `items[].action` MUST correspond to a Command name defined in the `protocol` crate. A CI check enforces this at build time.
- No shortcut binding may appear in C++ source. The registry is the single source of truth; the shell reads it at startup and constructs `QShortcut`/`QAction` objects from it.

**Alternatives considered.** (a) **JSON:** no comment support — inline rationale for non-obvious bindings (e.g., why `Alt+Left` for previous-view) cannot be documented in the file; brace noise degrades diff legibility. Rejected. (b) **YAML:** admits implicit type coercion hazards (the "Norway problem" and others); no C++ parser matches `toml11` in ergonomics and auditability. Rejected. (c) **XML:** verbose; angle-bracket noise dominates diffs over semantic changes; heavyweight for a configuration file. Rejected. (d) **QSettings / INI:** flat key-value structure cannot represent menu hierarchy without encoding tricks; no native comment support. Rejected. (e) **Custom DSL:** unnecessary maintenance burden and contributor friction for a file with a stable, bounded schema. Rejected. (f) **Rust-generated C++ header:** a Rust build step that parses TOML and emits C++ constants would add cross-language build coupling for no gain over `toml11` reading the same file directly. Rejected.

**Trade-offs.** `toml11` is a new C++ Tier-2 dependency (`[ADR-028]`): MIT license, header-only, no linking obligations, actively maintained. TOML's array-of-tables syntax (`[[menus]]`) is unfamiliar to developers who know only JSON/YAML; the format requires a one-time read of the TOML spec. Changes to menu labels are `profile_version` bumps — even pure copy changes — which adds friction to wording tweaks; this is intentional (label changes affect keyboard-navigation mnemonics and are stability-contract changes).

**Dependency record (`[ADR-028]` DEP-1).** `toml11` v4 is MIT-licensed (allowlisted) and is recorded in `third_party/MANIFEST.md` with pinned version, upstream source, and exit strategy. Header-only; no shared-library linking obligation. Exit path: replacing the parser requires only changes within `shell/chrome/`; the TOML registry format and `profile_version` contract are unaffected.

**Consequences.** Every shortcut or menu change produces a readable, attributable one-line diff in `ui-registry.toml`, making stability-contract review mechanical. The `profile_version` field is the ADR-030 anchor for org-policy pinning and for the "what changed and how to revert" in-product diff notes. A CI gate that diffs `profile_version` against the prior tagged profile detects accidental omissions of a version bump. The action-id ↔ protocol-Command validation at build time ensures the registry and the protocol crate stay in sync, preventing silent mismatches between the UI and the command layer.

**Future considerations.** Plugin-contributed menu items and tool entries (`[ADR-014]`) are contributed at runtime via events and do not modify this file; they are not part of the stability contract since they are external to the core product. If localization is needed beyond Qt Linguist, the labels in this file become translation keys; `toml11` read + Qt translation lookup is the natural integration point. A machine-readable changelog of `profile_version` diffs, published alongside release notes, is a future CI artifact.

---

*Accepted. Scaffolding of `shell/chrome/` and shortcut/action-mapping components may proceed.*
