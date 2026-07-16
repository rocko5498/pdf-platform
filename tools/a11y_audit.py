#!/usr/bin/env python3
"""
Automated a11y static audit for the Qt shell. [NFR-A11Y, DS-A11Y-*, DS §13]

Exit 0 = all gates pass. Exit 1 = missing required patterns.
This does not replace a real screen-reader task audit; it blocks regressions
in labels, roles, and focus policies committed to the design system.
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
    ("**/ui-registry.toml", r"open_document|focus_canvas", "shortcut registry stability"),
    ("**/main.cc", r"installAccessibility", "app installs a11y at startup"),
]

# Anti-patterns (fail if found in production sources)
ANTI = [
    (r"setAccessibleName\s*\(\s*\"\"\s*\)", "empty accessible name"),
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


if __name__ == "__main__":
    sys.exit(main())
