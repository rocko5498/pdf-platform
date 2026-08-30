#!/usr/bin/env python3
"""Find public items whose only callers are their own tests.

Four defects this year had the same shape: code that was written, tested, and
never called by the product.

  * `reconstruct_xref` — SDS §10.4's repair path, implemented at M0, wired in on
    2026-08-23. A damaged startxref failed to open for months.
  * The OCR job's dispatch arm — the worker had no match arm, so every real OCR
    job answered "unsupported operation".
  * `MergeCommand` / `SplitCommand` / `OptimizeCommand` — three `Command`
    implementations whose `apply` was an empty `Ok(())`.
  * `coordinator::render::RenderLoop` — SDS §6's scheduler, prefetch and bounded
    tile cache, bypassed by the shell's own compositor.

Each was found by reading code. That is not a gate. This is: a public item that
nothing outside its own file mentions is either dead or about to be, and saying
so out loud is cheaper than discovering it a milestone later.

Deliberate exceptions live in `tools/dead_api_allow.txt`, one identifier per
line with a reason after `#`. An entry there is a claim someone made on the
record, which is the point.  [ADR-039 EV-4, GR-8, PRIN-6, T-11]
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

DECLARATION = re.compile(
    r"^\s*pub(?:\s*\([^)]*\))?\s+"
    r"(?:async\s+|unsafe\s+|extern\s+\"[^\"]*\"\s+)*"
    r"(fn|struct|enum|trait|const|static)\s+"
    r"([A-Za-z_][A-Za-z0-9_]*)"
)

# Identifiers that are conventional and would drown the report.
CONVENTIONAL = {
    "new",
    "default",
    "from",
    "into",
    "len",
    "is_empty",
    "fmt",
    "clone",
    "eq",
    "hash",
    "next",
    "drop",
    "main",
}


def strip_tests(source: str) -> str:
    """Remove `#[cfg(test)] mod ... { ... }` blocks, braces balanced."""
    out = []
    index = 0
    while True:
        marker = source.find("#[cfg(test)]", index)
        if marker == -1:
            out.append(source[index:])
            return "".join(out)
        out.append(source[index:marker])
        brace = source.find("{", marker)
        if brace == -1:
            return "".join(out)
        depth = 0
        cursor = brace
        while cursor < len(source):
            if source[cursor] == "{":
                depth += 1
            elif source[cursor] == "}":
                depth -= 1
                if depth == 0:
                    break
            cursor += 1
        index = cursor + 1


def rust_sources(root: Path) -> list[Path]:
    files = []
    for path in sorted(root.rglob("*.rs")):
        parts = set(path.parts)
        if "target" in parts or "third_party" in parts:
            continue
        files.append(path)
    return files


def load_allowlist(path: Path) -> dict[str, str]:
    allowed: dict[str, str] = {}
    if not path.exists():
        return allowed
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        name, _, reason = line.partition("#")
        allowed[name.strip()] = reason.strip()
    return allowed


def main() -> int:
    root = Path(__file__).resolve().parent.parent
    allowlist = load_allowlist(root / "tools" / "dead_api_allow.txt")

    sources = rust_sources(root)
    bodies = {path: strip_tests(path.read_text(encoding="utf-8", errors="replace"))
              for path in sources}

    # Where each public item is declared.
    declarations: dict[str, Path] = {}
    for path, body in bodies.items():
        # Benchmarks and test crates declare helpers nothing else calls; that is
        # what they are for.
        if "benchmarks" in path.parts or "tests" in path.parts:
            continue
        for line in body.splitlines():
            match = DECLARATION.match(line)
            if not match:
                continue
            name = match.group(2)
            if name in CONVENTIONAL or name.startswith("_"):
                continue
            declarations.setdefault(name, path)

    # Every mention anywhere, including tests and benchmarks: a name used only
    # by its own unit tests still counts as unused *by the product*, so tests in
    # the declaring file do not count, but any other file does.
    mentions: dict[str, set[Path]] = {name: set() for name in declarations}
    word = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")
    for path, body in bodies.items():
        text = path.read_text(encoding="utf-8", errors="replace")
        for token in set(word.findall(text)):
            if token in mentions:
                mentions[token].add(path)

    unused = []
    for name, declared_in in sorted(declarations.items()):
        elsewhere = mentions[name] - {declared_in}
        if elsewhere:
            continue
        if name in allowlist:
            continue
        unused.append(f"{declared_in.relative_to(root)}: `{name}` is public and "
                      f"mentioned in no other file")

    for line in unused:
        print(line)
    print(f"{len(sources)} Rust files scanned; {len(declarations)} public items considered; "
          f"{len(allowlist)} allowed by name")

    if unused:
        print(f"FAIL: {len(unused)} public item(s) nothing outside their own file mentions")
        print("Wire it in, delete it, or add it to tools/dead_api_allow.txt with a reason.")
        return 1
    print("OK: every public item is mentioned somewhere outside its own file")
    return 0


if __name__ == "__main__":
    sys.exit(main())
