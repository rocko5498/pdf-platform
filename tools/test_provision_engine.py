#!/usr/bin/env python3
"""Checks for `provision_engine.py`. [ADR-022 T-1, ADR-028 §1]

    python tools/test_provision_engine.py

Covers the three properties a supply-chain step is worth having: the pinned
manifest parses and is complete, a checksum mismatch installs nothing, and an
unpinned platform fails with an actionable message instead of guessing.
"""

import hashlib
import io
import sys
import tarfile
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import provision_engine as pe  # noqa: E402

PLATFORM_IDS = {"win-x64", "linux-x64", "mac-x64", "mac-arm64"}


def test_manifest_is_complete() -> None:
    manifest = pe.load_manifest()
    assert manifest["license"] == "BSD-3-Clause", manifest["license"]
    assert manifest["upstream_ref"].startswith("chromium/"), manifest["upstream_ref"]
    assert PLATFORM_IDS <= set(manifest["platform"]), manifest["platform"].keys()
    for name, entry in manifest["platform"].items():
        assert len(entry["sha256"]) == 64, f"{name}: sha256 must be a full digest"
        assert int(entry["sha256"], 16) >= 0, f"{name}: sha256 must be hex"
        assert entry["archive"].endswith(".tgz"), name
        assert entry["library"], name
    # Every CI and dev platform the workflow builds on must be pinned, or the
    # build falls back to "no engine" and the M0 tile criterion cannot be shown.
    assert "linux-x64" in manifest["platform"], "ubuntu-latest runner"
    assert "mac-arm64" in manifest["platform"], "macos-latest runner is arm64"


def test_url_is_built_from_the_pinned_ref() -> None:
    manifest = pe.load_manifest()
    url = manifest["url_template"].format(
        ref=manifest["upstream_ref"], archive=manifest["platform"]["linux-x64"]["archive"]
    )
    assert url.startswith("https://"), url
    assert manifest["upstream_ref"] in url, url


def _fake_archive(member: str, payload: bytes = b"not really a library") -> bytes:
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode="w:gz") as archive:
        info = tarfile.TarInfo(member)
        info.size = len(payload)
        archive.addfile(info, io.BytesIO(payload))
    return buffer.getvalue()


def test_checksum_mismatch_installs_nothing(monkeypatched_root: Path) -> None:
    archive = _fake_archive("lib/libpdfium.so")
    pe.download = lambda url: archive  # noqa: ARG005 - stub
    try:
        pe.provision("linux-x64", force=True)
    except SystemExit as exit_error:
        message = str(exit_error)
        assert "SHA-256 mismatch" in message, message
        assert "Nothing was installed" in message, message
    else:
        raise AssertionError("a mismatched artifact must not install")
    assert not pe.install_dir("linux-x64").exists(), "no install directory on mismatch"


def test_unpinned_platform_is_actionable() -> None:
    try:
        pe.provision("solaris-sparc")
    except SystemExit as exit_error:
        message = str(exit_error)
        assert "no pinned PDFium artifact" in message, message
        assert "provenance.toml" in message, message
    else:
        raise AssertionError("an unpinned platform must fail loudly")


def test_matching_checksum_installs_the_library(monkeypatched_root: Path) -> None:
    archive = _fake_archive("lib/libpdfium.so")
    digest = hashlib.sha256(archive).hexdigest()
    real_load = pe.load_manifest

    def patched_load() -> dict:
        manifest = real_load()
        manifest["platform"]["linux-x64"]["sha256"] = digest
        return manifest

    pe.load_manifest = patched_load
    pe.download = lambda url: archive  # noqa: ARG005 - stub
    try:
        installed = pe.provision("linux-x64", force=True)
        assert installed.is_file(), installed
        assert installed.name == "libpdfium.so", installed
    finally:
        pe.load_manifest = real_load


def main() -> int:
    original_prebuilt = pe.PREBUILT
    original_download = pe.download
    failures = 0
    for name, test in sorted(globals().items()):
        if not name.startswith("test_"):
            continue
        with tempfile.TemporaryDirectory() as tmp:
            pe.PREBUILT = Path(tmp) / "prebuilt"
            try:
                if test.__code__.co_argcount:
                    test(pe.PREBUILT)
                else:
                    test()
                print(f"ok   {name}")
            except AssertionError as error:
                failures += 1
                print(f"FAIL {name}: {error}")
            finally:
                pe.PREBUILT = original_prebuilt
                pe.download = original_download
    print(f"{failures} failure(s)")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
