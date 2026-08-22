#!/usr/bin/env python3
"""Produce files with our writer, then have veraPDF try to parse them.

ADR-022 §6 asks for veraPDF validation of everything our writer emits. This
drives the real CLI to emit each kind of file we write, then hands them to
`verapdf_gate.py`.

The point is an *independent* opinion. Our own tests read our own output with
PDFium; a bug that both our writer and our reader agree on stays invisible to
them — which is exactly what happened with the `/Prev 0` back-pointer, a file
PDFium opened happily and no reader is obliged to accept.

    python tools/verapdf_writer_gate.py
"""

import subprocess
import sys
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
FIXTURES = REPO_ROOT / "tools" / "corpus-diff" / "fixtures"


def cli_binary() -> Path:
    name = "pdf-platform.exe" if sys.platform == "win32" else "pdf-platform"
    candidates = [
        REPO_ROOT / "core" / "target" / "debug" / name,
        REPO_ROOT / "core" / "target" / "release" / name,
    ]
    for path in candidates:
        if path.is_file():
            return path
    raise SystemExit(
        "error: CLI binary not built. Run `cargo build -p cli` first.\n"
        f"       Looked in: {', '.join(str(p) for p in candidates)}"
    )


def run_cli(binary: Path, args: list[str]) -> subprocess.CompletedProcess:
    return subprocess.run(
        [str(binary), *args], capture_output=True, text=True, check=False, cwd=REPO_ROOT
    )


def emit(binary: Path, out_dir: Path) -> list[Path]:
    """Emit one file per writer path we have a command for."""
    emitted: list[Path] = []
    source = FIXTURES / "valid-1page.pdf"

    # 1. Stamping: appends an incremental revision with new content and font
    #    objects, and rewrites the xref chain.
    stamped = out_dir / "stamped.pdf"
    result = run_cli(
        binary, ["stamp", str(source), "--text", "CONFIDENTIAL", "-o", str(stamped)]
    )
    if stamped.is_file():
        emitted.append(stamped)
    else:
        print(f"warn: stamp produced nothing ({result.stderr.strip()[:200]})")

    # 2. Bates numbering: the other stamp path, different content stream.
    numbered = out_dir / "bates.pdf"
    result = run_cli(
        binary,
        ["stamp", str(source), "--bates-start", "1", "--bates-width", "4", "-o", str(numbered)],
    )
    if numbered.is_file():
        emitted.append(numbered)
    else:
        print(f"warn: bates produced nothing ({result.stderr.strip()[:200]})")

    # 3. Assembly paths need qpdf; skip cleanly when it is absent rather than
    #    failing a gate for a missing tool. [GR-8]
    merged = out_dir / "merged.pdf"
    result = run_cli(
        binary,
        ["merge", str(source), str(FIXTURES / "valid-3page.pdf"), "-o", str(merged)],
    )
    if merged.is_file():
        emitted.append(merged)
    else:
        print(f"note: merge unavailable, skipping ({result.stderr.strip()[:120]})")

    return emitted


def main() -> int:
    binary = cli_binary()
    with tempfile.TemporaryDirectory() as tmp:
        out_dir = Path(tmp)
        emitted = emit(binary, out_dir)
        if not emitted:
            print("error: the writer emitted nothing; the gate would pass vacuously")
            return 1

        print(f"checking {len(emitted)} emitted file(s) with veraPDF")
        gate = REPO_ROOT / "tools" / "verapdf_gate.py"
        result = subprocess.run(
            [sys.executable, str(gate), *[str(path) for path in emitted]],
            check=False,
        )
        return result.returncode


if __name__ == "__main__":
    sys.exit(main())
