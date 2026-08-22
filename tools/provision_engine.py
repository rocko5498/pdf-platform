#!/usr/bin/env python3
"""Install the pinned PDFium prebuilt from `third_party/pdfium/provenance.toml`.

The engine is a setup-time input. Product code never fetches it: Z1 has no
network (GR-1) and nothing transmits without an explicit user action (GR-9),
so acquisition happens here, once, before a build. [ADR-028, ADR-029, SDS §13.4]

    python tools/provision_engine.py            # install for this host
    python tools/provision_engine.py --check    # report status, install nothing
    python tools/provision_engine.py --platform linux-x64

Exit codes: 0 installed or already present, 1 failure (bad hash, no artifact
for this platform, download error). A checksum mismatch is always a failure and
never falls back to the downloaded bytes.
"""

import argparse
import hashlib
import os
import platform
import shutil
import sys
import tarfile
import tempfile
import tomllib
import urllib.request
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
PDFIUM_DIR = REPO_ROOT / "third_party" / "pdfium"
MANIFEST = PDFIUM_DIR / "provenance.toml"
PREBUILT = PDFIUM_DIR / "prebuilt"


def host_platform() -> str:
    """Manifest platform id for the running host."""
    machine = platform.machine().lower()
    arm = machine in ("arm64", "aarch64")
    if sys.platform == "win32":
        return "win-arm64" if arm else "win-x64"
    if sys.platform == "darwin":
        return "mac-arm64" if arm else "mac-x64"
    return "linux-arm64" if arm else "linux-x64"


def load_manifest() -> dict:
    with MANIFEST.open("rb") as handle:
        return tomllib.load(handle)


def install_dir(platform_id: str) -> Path:
    return PREBUILT / platform_id


def library_path(manifest: dict, platform_id: str) -> Path:
    """Where the shared library lands once installed."""
    entry = manifest["platform"][platform_id]
    return install_dir(platform_id) / Path(entry["library"]).name


def download(url: str) -> bytes:
    with urllib.request.urlopen(url) as response:  # noqa: S310 - fixed https URL from the manifest
        return response.read()


def provision(platform_id: str, force: bool = False) -> Path:
    manifest = load_manifest()
    platforms = manifest["platform"]
    if platform_id not in platforms:
        raise SystemExit(
            f"error: no pinned PDFium artifact for platform '{platform_id}'.\n"
            f"       {MANIFEST} pins: {', '.join(sorted(platforms))}.\n"
            "       Add the platform to the manifest with its SHA-256 before building here."
        )
    entry = platforms[platform_id]
    target = library_path(manifest, platform_id)
    if target.is_file() and not force:
        print(f"pdfium: already installed at {target}")
        return target

    url = manifest["url_template"].format(
        ref=manifest["upstream_ref"], archive=entry["archive"]
    )
    print(f"pdfium: fetching {entry['archive']} ({manifest['upstream_ref']})")
    payload = download(url)

    actual = hashlib.sha256(payload).hexdigest()
    if actual != entry["sha256"]:
        raise SystemExit(
            f"error: SHA-256 mismatch for {entry['archive']}\n"
            f"       expected {entry['sha256']}\n"
            f"       actual   {actual}\n"
            "       Nothing was installed. Do not proceed: the artifact does not match the manifest."
        )

    member = entry["library"]
    # Extract and rename in one temp directory next to the destination, so a
    # half-written install is never visible to a concurrent build. The parallel
    # PDFium load flake came from unpacking straight onto a live path.
    PREBUILT.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=PREBUILT) as tmp:
        tmp_path = Path(tmp)
        archive_path = tmp_path / entry["archive"]
        archive_path.write_bytes(payload)
        staged = tmp_path / "staged"
        staged.mkdir()
        with tarfile.open(archive_path) as archive:
            names = archive.getnames()
            if member not in names:
                raise SystemExit(
                    f"error: {entry['archive']} does not contain {member}; manifest is stale"
                )
            for name in (member, "LICENSE"):
                if name in names:
                    archive.extract(name, staged, filter="data")
        final = install_dir(platform_id)
        if final.exists():
            shutil.rmtree(final)
        (staged / member).parent.mkdir(parents=True, exist_ok=True)
        # Flatten: the library sits directly in <platform>/, license beside it.
        payload_dir = tmp_path / "install"
        payload_dir.mkdir()
        shutil.move(str(staged / member), str(payload_dir / Path(member).name))
        license_file = staged / "LICENSE"
        if license_file.is_file():
            shutil.move(str(license_file), str(payload_dir / "LICENSE"))
        os.replace(payload_dir, final)

    print(f"pdfium: installed {target}")
    return target


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--platform", default=host_platform(), help="manifest platform id")
    parser.add_argument("--check", action="store_true", help="report status only")
    parser.add_argument("--force", action="store_true", help="reinstall even if present")
    args = parser.parse_args()

    if args.check:
        manifest = load_manifest()
        if args.platform not in manifest["platform"]:
            print(f"pdfium: no artifact pinned for {args.platform}")
            return 1
        target = library_path(manifest, args.platform)
        if target.is_file():
            print(f"pdfium: present at {target}")
            return 0
        print(f"pdfium: missing; run `python tools/provision_engine.py` to install {target}")
        return 1

    provision(args.platform, force=args.force)
    return 0


if __name__ == "__main__":
    sys.exit(main())
