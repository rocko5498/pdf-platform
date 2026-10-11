#!/usr/bin/env python3
"""Open every PDF in a corpus through the product and compare with qpdf.

Roadmap 3.3 asks for a real-world corpus for open and open-edit-save. This is
the open half: for each file it asks qpdf for the page count, asks the product
(`pdf-platform diagnostics`, which goes through the worker and PDFium, the path
a user's document takes) for its page count, and reports

    match               the two agree
    MISMATCH            both opened and disagree
    we_open_qpdf_fails  we opened a file qpdf rejects (engine is more lenient)
    we_fail_qpdf_ok     qpdf opened a file we could not   <- the one to fix
    both_fail           neither opened it
    TIMEOUT / CRASH     the process hung or died          <- never acceptable

Nothing here claims fidelity: a matching page count says the structure was read,
not that the page renders correctly. The corpus is not in this repository; the
numbers recorded in docs/milestone-exit-tracker.md say which commit of which
corpus they came from.

    git clone --depth 1 --filter=blob:none --sparse https://github.com/mozilla/pdf.js.git pdfjs
    (cd pdfjs && git sparse-checkout set test/pdfs)
    cargo build -p cli -p worker-main        # in core/
    python tools/corpus_sweep.py pdfjs/test/pdfs [--jobs 6] [--json out.json]

Exit 0 when nothing timed out or crashed and no file qpdf opens failed to open;
1 otherwise. [roadmap 3.3, MET-FEAT-1, MET-REL-4, T-13]
"""
import argparse
import collections
import concurrent.futures as cf
import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def cli_binary() -> Path:
    name = "pdf-platform.exe" if sys.platform == "win32" else "pdf-platform"
    for flavour in ("debug", "release"):
        path = ROOT / "core" / "target" / flavour / name
        if path.is_file():
            return path
    raise SystemExit("error: CLI not built; run `cargo build -p cli -p worker-main` in core/")


def run(cmd, timeout):
    try:
        r = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=timeout)
        return r.returncode, r.stdout, r.stderr
    except subprocess.TimeoutExpired:
        return "timeout", "", ""


def classify(exe, qpdf, path, timeout):
    qc, qo, _ = run([qpdf, "--show-npages", str(path)], timeout)
    q = int(qo.strip()) if qc in (0, 3) and qo.strip().isdigit() else None
    oc, oo, oe = run([str(exe), "diagnostics", str(path)], timeout)
    m = re.search(r"^Pages:\s+(\d+)", oo, re.M)
    ours = int(m.group(1)) if m else None
    err = next((line for line in oe.splitlines() if line.startswith("error")), "")[:100]
    if oc == "timeout":
        return "TIMEOUT", path.name
    if isinstance(oc, int) and oc not in (0, 1, 2, 3):
        return f"CRASH exit={oc}", path.name
    if ours is None:
        return ("we_fail_qpdf_ok" if q is not None else "both_fail"), (path.name, err)
    if q is None:
        return "we_open_qpdf_fails", (path.name, ours)
    return ("match", path.name) if ours == q else ("MISMATCH", (path.name, ours, q))


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("corpus", type=Path, help="directory of .pdf files")
    ap.add_argument("--jobs", type=int, default=6)
    ap.add_argument("--timeout", type=int, default=90)
    ap.add_argument("--json", type=Path, help="write the full classification here")
    args = ap.parse_args()

    qpdf = shutil.which("qpdf")
    if qpdf is None:
        raise SystemExit("error: qpdf not on PATH (it is the oracle)")
    exe = cli_binary()
    files = sorted(p for p in args.corpus.glob("*.pdf"))
    if not files:
        raise SystemExit(f"error: no .pdf files in {args.corpus}")

    cat = collections.defaultdict(list)
    with cf.ThreadPoolExecutor(max_workers=args.jobs) as pool:
        for kind, item in pool.map(lambda p: classify(exe, qpdf, p, args.timeout), files):
            cat[kind].append(item)

    print({k: len(v) for k, v in sorted(cat.items())}, "total", len(files))
    for kind in sorted(cat):
        if kind not in ("match", "we_open_qpdf_fails"):
            for item in cat[kind][:50]:
                print(f"  {kind}: {item}")
    if args.json:
        args.json.write_text(json.dumps(cat, indent=1), encoding="utf-8")
    bad = [k for k in cat if k == "TIMEOUT" or k.startswith("CRASH") or k == "we_fail_qpdf_ok"]
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
