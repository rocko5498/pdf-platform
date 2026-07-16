#!/usr/bin/env python3
"""
Validate p95 gate definition file and print the release checklist. [ADR-023]

Does **not** invent benchmark numbers. CI uses this to ensure the gate table
exists and is well-formed; measured results are attached at release from
`cargo bench -p benchmarks` on reference hardware.
"""
from __future__ import annotations

import re
import sys
from pathlib import Path

GATES = Path(__file__).resolve().parent / "p95_gates.toml"


def main() -> int:
    text = GATES.read_text(encoding="utf-8")
    if "cold_start_ms" not in text or "first_page_ms" not in text:
        print("FAIL: required gates missing from p95_gates.toml")
        return 1
    budgets = re.findall(r"budget\s*=\s*(\d+)", text)
    if not budgets:
        print("FAIL: no budgets defined")
        return 1
    print(f"OK: {GATES.name} defines {len(budgets)} budget entries")
    print("Release procedure:")
    print("  1. cargo bench -p benchmarks --bench startup")
    print("  2. cargo bench -p benchmarks --bench large_doc  # if configured")
    print("  3. Compare p95 to tools/bench/p95_gates.toml on reference hardware")
    print("  4. Attach results to release notes; exceed budget => block release")
    return 0


if __name__ == "__main__":
    sys.exit(main())
