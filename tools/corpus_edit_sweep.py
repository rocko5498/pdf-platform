#!/usr/bin/env python3
"""Edit every PDF in a corpus through the product and let qpdf judge the result.

The edit half of roadmap 3.3 (open-edit-save). For each file qpdf can count, run
rotate, crop, duplicate-pages, insert-blank and (if it has two pages or more)
delete-pages on page 1, then classify each attempt:

    ok              exit 0, qpdf --check passes (exit 0 or 3), page count as expected
    refused         exit 1 or 2 with an `error` message: the product said no, honestly
    damaged_source  exit 0 and the output fails qpdf's check, but so does the source
                    (check exit 2): the damage was inherited, not caused, and is
                    reported separately so it is not hidden
    BAD_OUTPUT      exit 0, the source was fine (qpdf check 0 or 3), and the output
                    fails the check or has the wrong page count
                    <- a "successful" save that damaged the document; never acceptable
    CRASH/TIMEOUT   the process died or hung                     <- never acceptable
    silent_fail     non-zero exit with no `error` message        <- a GR-8 breach

Refusals are not failures: a file the product cannot safely edit should be
refused with a reason. What this measures is that every "yes" is a real yes.

    python tools/corpus_edit_sweep.py pdfjs/test/pdfs [--jobs 6] [--json out.json]

Exit 0 when there is no BAD_OUTPUT, CRASH, TIMEOUT or silent_fail.
[roadmap 3.3, MET-FEAT-1, MET-REL-2, MET-REL-4, GR-8, T-13]
"""
import argparse
import collections
import concurrent.futures as cf
import json
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OPS = {
    "rotate": (["rotate", "{src}", "--pages", "1", "--degrees", "90", "-o", "{out}"], lambda n: n),
    "crop": (["crop", "{src}", "--pages", "1", "--box", "10,10,300,400", "-o", "{out}"], lambda n: n),
    "duplicate": (["duplicate-pages", "{src}", "--pages", "1", "-o", "{out}"], lambda n: n + 1),
    "blank": (["insert-blank", "{src}", "--at", "1", "-o", "{out}"], lambda n: n + 1),
    "delete": (["delete-pages", "{src}", "--pages", "1", "-o", "{out}"], lambda n: n - 1),
    "move": (["move-pages", "{src}", "--pages", "2", "--to", "1", "-o", "{out}"], lambda n: n),
    "extract": (["extract-pages", "{src}", "1", "1", "-o", "{out}"], lambda n: 1),
    "stamp": (["stamp", "{src}", "--text", "DRAFT", "-o", "{out}"], lambda n: n),
    "sanitize": (["sanitize", "{src}", "-o", "{out}"], lambda n: n),
}
# Operations that need more than one page to mean anything.
MIN_PAGES = {"delete": 2, "move": 2}


def cli_binary() -> Path:
    name = "pdf-platform.exe" if sys.platform == "win32" else "pdf-platform"
    for flavour in ("debug", "release"):
        path = ROOT / "core" / "target" / flavour / name
        if path.is_file():
            return path
    raise SystemExit("error: CLI not built; run `cargo build -p cli -p worker-main` in core/")


def run(cmd, timeout):
    try:
        r = subprocess.run([str(c) for c in cmd], capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=timeout)
        return r.returncode, r.stdout, r.stderr
    except subprocess.TimeoutExpired:
        return "timeout", "", ""


def npages(qpdf, path, timeout):
    code, out, _ = run([qpdf, "--show-npages", path], timeout)
    return int(out.strip()) if code in (0, 3) and out.strip().isdigit() else None


def attempt(exe, qpdf, src, op, workdir, timeout):
    args, expect = OPS[op]
    n = npages(qpdf, src, timeout)
    # A page count in the billions is qpdf describing a fuzzed file, not a
    # document anyone could compare against.
    if n is None or n < 1 or n > 100_000 or n < MIN_PAGES.get(op, 1):
        return None
    out = workdir / f"{src.stem}-{op}.pdf"
    cmd = [exe] + [a.format(src=src, out=out) for a in args]
    code, _, err = run(cmd, timeout)
    try:
        if code == "timeout":
            return ("TIMEOUT", src.name, op)
        if isinstance(code, int) and code not in (0, 1, 2, 3):
            return (f"CRASH exit={code}", src.name, op)
        if code != 0:
            # The message can follow worker stderr on the same line.
            message = next((line for line in err.splitlines() if "error" in line), None)
            if message is not None:
                return ("refused", src.name, op, message[message.find("error"):][:90])
            return ("silent_fail", src.name, op, err.strip()[-80:])
        if not out.exists():
            return ("BAD_OUTPUT", src.name, op, "exit 0 but no file written")
        check_code, _, _ = run([qpdf, "--check", out], timeout)
        got = npages(qpdf, out, timeout)
        if check_code in (0, 3) and got == expect(n):
            return ("ok", src.name, op)
        source_code, _, _ = run([qpdf, "--check", src], timeout)
        detail = f"qpdf check={check_code} (source {source_code}) pages={got} want={expect(n)}"
        if source_code == 2 and got == expect(n):
            return ("damaged_source", src.name, op, detail)
        return ("BAD_OUTPUT", src.name, op, detail)
    finally:
        out.unlink(missing_ok=True)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("corpus", type=Path)
    ap.add_argument("--jobs", type=int, default=6)
    ap.add_argument("--timeout", type=int, default=120)
    ap.add_argument("--json", type=Path)
    args = ap.parse_args()
    qpdf = shutil.which("qpdf")
    if qpdf is None:
        raise SystemExit("error: qpdf not on PATH (it is the judge)")
    exe = cli_binary()
    files = sorted(args.corpus.glob("*.pdf"))
    if not files:
        raise SystemExit(f"error: no .pdf files in {args.corpus}")

    results = []
    with tempfile.TemporaryDirectory(prefix="corpus-edit-") as tmp, cf.ThreadPoolExecutor(args.jobs) as pool:
        jobs = [(f, op) for f in files for op in OPS]
        def go(job):
            f, op = job
            wd = Path(tmp) / f"{abs(hash((f.name, op)))}"
            wd.mkdir(exist_ok=True)
            return attempt(exe, qpdf, f, op, wd, args.timeout)
        for r in pool.map(go, jobs):
            if r:
                results.append(r)

    tally = collections.Counter(r[0] for r in results)
    per_op = collections.defaultdict(collections.Counter)
    for r in results:
        per_op[r[2]][r[0]] += 1
    print(dict(tally), "attempts", len(results))
    for op, c in per_op.items():
        print(f"  {op:10} {dict(c)}")
    bad = [r for r in results if r[0] not in ("ok", "refused", "damaged_source")]
    for r in bad[:60]:
        print("  ", r)
    reasons = collections.Counter(r[3] for r in results if r[0] == "refused")
    for reason, count in reasons.most_common(8):
        print(f"  refused x{count}: {reason}")
    if args.json:
        args.json.write_text(json.dumps(results, indent=1), encoding="utf-8")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
