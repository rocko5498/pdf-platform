#!/usr/bin/env python3
"""Catch `windows(N)` compared against a literal of a different length.

`slice::windows(N)` yields N-byte slices. Comparing one against a byte-string
literal of a different length is never equal, so the search silently never
matches and the code takes whatever fallback it has. This has shipped four
times in this repository:

  * `windows(7)` against `b"endobj"` (6) — every object fetched from the worker
    ran from its offset to end of file, which corrupted every stamp.
  * `windows(7)` against `b"/Flate"` (6) — every PDF stream was treated as
    having an unknown filter and handed on still compressed.
  * `windows(6)` against `b"/None"` (5) — same function, same shape.
  * `windows(11)` against `b"Secret Agent"` (12) — an assertion that could not
    fail, in the redaction suite.

The pattern is mechanical, so it is checked mechanically. [T-11, PRIN-1, GR-8]
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

# `windows(7)` … `b"endobj"` on the same line or the next few, which is how
# every instance so far has been written.
WINDOW = re.compile(r"\.windows\((\d+)\)")
LITERAL = re.compile(r'b"((?:[^"\\]|\\.)*)"')

# How many lines after the `windows(N)` a comparison literal may appear.
LOOKAHEAD = 4


def literal_length(text: str) -> int:
    """Byte length of a Rust byte-string literal's contents."""
    length = 0
    index = 0
    while index < len(text):
        if text[index] == "\\" and index + 1 < len(text):
            following = text[index + 1]
            if following == "x" and index + 3 < len(text):
                index += 4  # \xNN
            elif following == "u" and index + 2 < len(text) and text[index + 2] == "{":
                end = text.index("}", index) + 1
                index = end
            else:
                index += 2
            length += 1
            continue
        index += 1
        length += 1
    return length


def check_file(path: Path) -> list[str]:
    problems: list[str] = []
    lines = path.read_text(encoding="utf-8", errors="replace").splitlines()

    for number, line in enumerate(lines):
        # A comment explaining a past defect is not a defect.
        if line.lstrip().startswith("//"):
            continue

        window_text = chr(10).join(lines[number : number + 1 + LOOKAHEAD])
        for window in WINDOW.finditer(line):
            size = int(window.group(1))
            # Look only as far as the next `windows(` call: two comparisons on
            # one line are common, and each owns the literal that follows it.
            span_start = window.end()
            next_window = WINDOW.search(window_text, span_start)
            span_end = next_window.start() if next_window else len(window_text)
            span = window_text[span_start:span_end]

            literal = LITERAL.search(span)
            if not literal:
                continue
            before = span[max(0, literal.start() - 6) : literal.start()]
            if "==" not in before and "!=" not in before:
                continue
            actual = literal_length(literal.group(1))
            if actual != size:
                problems.append(
                    f"{path}:{number + 1}: windows({size}) compared with "
                    f'b"{literal.group(1)}" ({actual} bytes) - never equal'
                )

    return problems


def main() -> int:
    root = Path(__file__).resolve().parent.parent
    problems: list[str] = []
    scanned = 0

    for path in sorted(root.rglob("*.rs")):
        if "target" in path.parts or "third_party" in path.parts:
            continue
        scanned += 1
        problems.extend(check_file(path))

    for problem in problems:
        print(problem)
    print(f"{scanned} Rust files scanned for window/needle length mismatches")

    if problems:
        print(f"FAIL: {len(problems)} mismatch(es)")
        return 1
    print("OK: every windows(N) comparison matches its needle's length")
    return 0


if __name__ == "__main__":
    sys.exit(main())
