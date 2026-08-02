#!/usr/bin/env python3
"""
Automated a11y static audit for the Qt shell. [NFR-A11Y, DS-A11Y-*, DS §13]

Exit 0 = all gates pass. Exit 1 = missing required patterns.

What this is: a presence check. Each gate greps one file for one pattern and
passes if it appears anywhere in it. That catches the regression where an
accessibility call is deleted outright, which is worth catching.

What this is NOT, so that a green run is not read as more than it is:

* It does not verify accessible *roles*. Nothing here inspects a QAccessible
  role, only that the header naming the types exists.
* It does not verify that a name is attached to the right widget, or that the
  name is meaningful. `setAccessibleName(QStringLiteral("x"))` passes.
* It does not verify focus *order* (AQA/RQA), only that a focus policy and a
  key handler exist somewhere in canvas.cc.
* It does not replace a screen-reader task audit. NFR-A11Y-3 makes
  accessibility regressions release-blocking, and that judgement needs a
  human with a screen reader (docs/a11y-audit-checklist.md).

Gate descriptions below must say what the pattern proves, not what a reader
would like it to prove. [PRIN-6, GR-8]
"""
from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SHELL = ROOT / "shell"

# Required patterns: (glob relative to shell, regex, description)
GATES = [
    ("**/a11y.cc", r"installAccessibility|QAccessible", "a11y factory / QAccessible"),
    ("**/a11y.h", r"CanvasAccessible|configureMainWindowAccessibility", "accessible canvas types"),
    ("**/canvas.cc", r"setAccessibleName|setFocusPolicy|StrongFocus", "canvas a11y + focus"),
    ("**/canvas.cc", r"Key_PageDown|Key_F6|keyPressEvent", "keyboard navigation path"),
    ("**/outline_panel.cc", r"setAccessibleName", "outline panel name"),
    ("**/diagnostics_panel.cc", r"setAccessibleName", "diagnostics panel name"),
    ("**/annotation_tools.cc", r"setAccessibleName", "annotation toolbar name"),
    ("**/forms_panel.cc", r"setAccessibleName", "forms panel name"),
    ("**/forms_panel.cc", r"setAccessibleDescription", "forms panel description"),
    ("**/canvas.cc", r"documentStatus|announceDocumentStatus|ValueChanged", "page status a11y announce"),
    # This does NOT check stability, and must not be described as if it did.
    # It greps ui-registry.toml for two identifiers, so it fires only if the
    # registry is deleted or gutted. RQA-1/RQA-2 stability means shortcuts and
    # menu taxonomy do not change between releases, which needs a comparison
    # against a recorded baseline that does not exist here.
    #
    # It also says nothing about the registry being *honoured*. On this tree it
    # is not: shell/canvas/canvas.cc contains 23 hard-coded Qt key references,
    # and no C++ file reads ui-registry.toml at all — shell/chrome/ has no .cc.
    # ADR-032 states "no shortcut binding may appear in C++ source". Enforcing
    # that needs a real conformance gate, not this line. [ADR-032, DS-CONV-4]
    ("**/ui-registry.toml", r"open_document|focus_canvas", "registry declares baseline action ids"),
    ("**/main.cc", r"installAccessibility", "app installs a11y at startup"),
]

# Anti-patterns (fail if found in production sources).
# An accessible name that is present but empty is worse than an absent one: it
# satisfies the presence gates above while giving a screen reader nothing.
# `QString()` and `QStringLiteral("")` produce exactly that and were not caught.
ANTI = [
    (
        r"setAccessible(?:Name|Description)\s*\(\s*(?:\"\"|QString\s*\(\s*\)"
        r"|QStringLiteral\s*\(\s*\"\"\s*\)|QLatin1String\s*\(\s*\"\"\s*\))\s*\)",
        "empty accessible name or description",
    ),
]


def main() -> int:
    failures: list[str] = []
    passes = 0

    for glob, pattern, desc in GATES:
        files = list(SHELL.glob(glob))
        if not files:
            failures.append(f"MISSING FILES: {glob} ({desc})")
            continue
        rx = re.compile(pattern)
        if any(rx.search(f.read_text(encoding="utf-8", errors="replace")) for f in files):
            passes += 1
            print(f"  OK  {desc}")
        else:
            failures.append(f"FAIL  {desc} — pattern /{pattern}/ not in {glob}")

    for f in SHELL.rglob("*"):
        if f.suffix not in {".cc", ".h", ".cpp", ".hpp"}:
            continue
        text = f.read_text(encoding="utf-8", errors="replace")
        for pattern, desc in ANTI:
            if re.search(pattern, text):
                failures.append(f"ANTI  {desc} in {f.relative_to(ROOT)}")

    print()
    print(f"a11y static audit: {passes} gates ok, {len(failures)} failures")
    for f in failures:
        print(f"  {f}")
    return 1 if failures else 0


def self_check() -> int:
    """Assert the anti-pattern catches what its description claims. `--self-check`."""
    rx = re.compile(ANTI[0][0])
    must_match = [
        'w->setAccessibleName("");',
        "w->setAccessibleName(QString());",
        'w->setAccessibleName(QStringLiteral(""));',
        'w->setAccessibleName( QLatin1String("") );',
        "w->setAccessibleDescription(QString());",
    ]
    must_not_match = [
        'w->setAccessibleName(QStringLiteral("Document canvas"));',
        'w->setAccessibleName(tr("Bookmarks"));',
        "w->setAccessibleName(name);",
    ]
    for s in must_match:
        assert rx.search(s), f"anti-pattern missed an empty name: {s}"
    for s in must_not_match:
        assert not rx.search(s), f"anti-pattern falsely flagged: {s}"
    print(f"self-check ok: {len(must_match)} caught, {len(must_not_match)} correctly ignored")
    return 0


if __name__ == "__main__":
    if "--self-check" in sys.argv:
        sys.exit(self_check())
    sys.exit(main())
