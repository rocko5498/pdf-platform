#!/usr/bin/env python3
"""Check the files our writer emits against veraPDF. [ADR-022 §6, CMP-STD-2]

ADR-022 §6 calls for "veraPDF validation of everything our writer emits".
Nothing did that, and `sign::validate_pdf_a` is byte-pattern heuristics that
say so themselves.

What this gate asserts is **structural acceptance by a recognized independent
implementation**, not PDF/A conformance: the product does not claim to emit
PDF/A, so reporting conformance violations as failures would be noise. A file
veraPDF cannot parse is a defect in our writer — that is the class the `/Prev 0`
back-pointer bug belonged to, where the file opened in PDFium and was malformed
all the same.

    python tools/verapdf_gate.py --provision-only
    python tools/verapdf_gate.py <file.pdf> [<file.pdf> ...]

Exit codes: 0 every file parsed, 1 a file failed to parse or veraPDF could not
be provisioned. Conformance violations are printed and do not fail the gate.
"""

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
import urllib.request
import zipfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
INSTALL_ROOT = REPO_ROOT / "third_party" / "verapdf" / "install"
INSTALLER_URL = "https://software.verapdf.org/releases/verapdf-installer.zip"

AUTO_INSTALL = """<?xml version="1.0" encoding="UTF-8" standalone="no"?>
<AutomatedInstallation langpack="eng">
  <com.izforge.izpack.panels.htmlhello.HTMLHelloPanel id="welcome"/>
  <com.izforge.izpack.panels.target.TargetPanel id="install_dir">
    <installpath>{install_path}</installpath>
  </com.izforge.izpack.panels.target.TargetPanel>
  <com.izforge.izpack.panels.packs.PacksPanel id="sdk_pack_select">
    <pack index="0" name="veraPDF Mac and *nix Scripts" selected="true"/>
    <pack index="1" name="veraPDF Validation model" selected="true"/>
    <pack index="2" name="veraPDF Documentation" selected="false"/>
    <pack index="3" name="veraPDF Sample Plugins" selected="false"/>
  </com.izforge.izpack.panels.packs.PacksPanel>
  <com.izforge.izpack.panels.install.InstallPanel id="install"/>
  <com.izforge.izpack.panels.finish.SimpleFinishPanel id="finish"/>
</AutomatedInstallation>
"""


def verapdf_binary() -> Path:
    name = "verapdf.bat" if sys.platform == "win32" else "verapdf"
    return INSTALL_ROOT / name


def provision() -> Path:
    """Install veraPDF if it is not already present. Returns the CLI path."""
    binary = verapdf_binary()
    if binary.is_file():
        print(f"verapdf: already installed at {binary}")
        return binary

    if shutil.which("java") is None:
        raise SystemExit(
            "error: veraPDF needs a JRE and `java` is not on PATH.\n"
            "       This is a test-environment dependency, never a product one:\n"
            "       nothing shipped to a user requires Java. [ADR-022 §6]"
        )

    print(f"verapdf: fetching {INSTALLER_URL}")
    with urllib.request.urlopen(INSTALLER_URL) as response:  # noqa: S310 - fixed https URL
        payload = response.read()
    print(f"verapdf: installer is {len(payload)} bytes, sha256 {hashlib.sha256(payload).hexdigest()}")

    INSTALL_ROOT.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as tmp:
        tmp_path = Path(tmp)
        archive = tmp_path / "verapdf-installer.zip"
        archive.write_bytes(payload)
        with zipfile.ZipFile(archive) as zf:
            zf.extractall(tmp_path)

        jars = sorted(tmp_path.glob("*/verapdf-izpack-installer-*.jar"))
        if not jars:
            raise SystemExit("error: no veraPDF installer jar inside the archive")
        auto = tmp_path / "auto-install.xml"
        auto.write_text(AUTO_INSTALL.format(install_path=INSTALL_ROOT), encoding="utf-8")

        result = subprocess.run(
            ["java", "-jar", str(jars[0]), str(auto)],
            capture_output=True,
            text=True,
            check=False,
        )
        if result.returncode != 0 or not verapdf_binary().is_file():
            raise SystemExit(
                "error: veraPDF install failed\n"
                f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}"
            )

    binary = verapdf_binary()
    if not sys.platform == "win32":
        os.chmod(binary, 0o755)
    print(f"verapdf: installed {binary}")
    return binary


def validate(binary: Path, files: list[Path]) -> int:
    """Run veraPDF over `files`; fail only on files it cannot parse."""
    command = [str(binary), "--format", "json", "--flavour", "0"]
    command += [str(path) for path in files]
    result = subprocess.run(command, capture_output=True, text=True, check=False)

    if not result.stdout.strip():
        print("error: veraPDF produced no report")
        print(result.stderr)
        return 1

    try:
        report = json.loads(result.stdout)
    except json.JSONDecodeError:
        print("error: veraPDF report was not JSON")
        print(result.stdout[:2000])
        return 1

    failures = 0
    jobs = report.get("report", {}).get("jobs", []) or report.get("jobs", [])
    for job in jobs:
        name = job.get("itemDetails", {}).get("name", "?")
        task_result = job.get("taskResult") or {}
        parse_error = task_result.get("exceptionMessage")
        validation = job.get("validationResult") or {}

        if parse_error:
            print(f"FAIL {name}: veraPDF could not parse it: {parse_error}")
            failures += 1
            continue

        # veraPDF reports one entry per flavour it validated against, and
        # emits a list when there is more than one. Normalise both shapes
        # rather than assuming the one this machine happened to produce.
        results = validation if isinstance(validation, list) else [validation]
        summaries = []
        for entry in results:
            if not isinstance(entry, dict):
                continue
            flavour = entry.get("profileName", "unknown profile")
            details = entry.get("details", {}) or {}
            failed_rules = details.get("failedRules", 0)
            state = "compliant" if entry.get("compliant") else f"{failed_rules} rule(s) not met"
            summaries.append(f"{flavour}: {state}")
        conformance = "; ".join(summaries) if summaries else "no conformance profile applied"
        print(f"ok   {name}: parsed. conformance ({conformance})")

    if not jobs:
        print("error: veraPDF reported no jobs; nothing was checked")
        return 1

    print()
    if failures:
        print(f"FAIL: {failures} file(s) a recognized validator could not parse")
        return 1
    print(f"OK: {len(jobs)} file(s) parsed by veraPDF")
    print("Note: conformance violations above are informational — the product")
    print("does not claim to emit PDF/A. [MET-FEAT-3, PRIN-6]")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("files", nargs="*", type=Path, help="PDFs to check")
    parser.add_argument("--provision-only", action="store_true")
    args = parser.parse_args()

    binary = provision()
    if args.provision_only:
        return 0
    if not args.files:
        parser.error("no files given")
    missing = [path for path in args.files if not path.is_file()]
    if missing:
        raise SystemExit(f"error: missing input(s): {', '.join(str(p) for p in missing)}")
    return validate(binary, args.files)


if __name__ == "__main__":
    sys.exit(main())
