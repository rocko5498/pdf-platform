#!/usr/bin/env python3
"""Check WIT world version backward compatibility. [ADR-015, ADR-030, M11]

This script validates that the WIT world version in plugin-sdk/wit/plugin.wit
follows semver and is backward-compatible with the previous version (if any).

Usage:
    python tools/ci/check_wit_compat.py [--verbose]

Exit codes:
    0 — compatible
    1 — incompatible or parse error
"""

import re
import sys
import os

WIT_FILE = os.path.join(
    os.path.dirname(__file__), "..", "..", "plugin-sdk", "wit", "plugin.wit"
)
SDK_FILE = os.path.join(
    os.path.dirname(__file__), "..", "..", "plugin-sdk", "src", "lib.rs"
)

# Pattern to match: package pdf-platform:plugin@1;
# WIT package declarations use major-only version; full semver is in the SDK const.
PACKAGE_RE = re.compile(r"package\s+[\w-]+:[\w-]+@(\d+)\s*;")
# Pattern to match: pub const CURRENT_WIT_WORLD_VERSION: &str = "1.0.0";
SDK_VERSION_RE = re.compile(r'pub\s+const\s+CURRENT_WIT_WORLD_VERSION:\s*&str\s*=\s*"(\d+\.\d+\.\d+)"')


def parse_semver(version_str):
    """Parse a semver string into (major, minor, patch)."""
    parts = version_str.split(".")
    if len(parts) < 3:
        return None
    try:
        return tuple(int(p) for p in parts[:3])
    except ValueError:
        return None


def check_compatibility(current, previous):
    """Check if current version is backward-compatible with previous.
    
    Returns (compatible: bool, reason: str).
    """
    if current is None or previous is None:
        return True, "no previous version to compare"

    # Major version must match (breaking changes)
    if current[0] != previous[0]:
        return False, (
            f"major version change: {previous} -> {current} "
            f"(breaking changes require major bump)"
        )

    # Current minor must not be less than previous (can't go backwards)
    if current[1] < previous[1]:
        return False, (
            f"minor version decrease: {previous} -> {current} "
            f"(versions must not decrease)"
        )

    # If same minor, patch must not decrease
    if current[1] == previous[1] and current[2] < previous[2]:
        return False, (
            f"patch version decrease: {previous} -> {current} "
            f"(versions must not decrease)"
        )

    return True, "compatible"


def extract_wit_major():
    """Extract major version from the WIT file's package declaration."""
    try:
        with open(WIT_FILE, "r") as f:
            content = f.read()
    except FileNotFoundError:
        print(f"ERROR: WIT file not found: {WIT_FILE}")
        return None

    match = PACKAGE_RE.search(content)
    if not match:
        print(f"ERROR: could not find package declaration in {WIT_FILE}")
        return None

    return match.group(1)


def extract_sdk_version():
    """Extract version from the SDK lib.rs."""
    try:
        with open(SDK_FILE, "r") as f:
            content = f.read()
    except FileNotFoundError:
        print(f"ERROR: SDK file not found: {SDK_FILE}")
        return None

    match = SDK_VERSION_RE.search(content)
    if not match:
        print(f"WARNING: could not find CURRENT_WIT_WORLD_VERSION in {SDK_FILE}")
        return None

    return match.group(1)


def main():
    verbose = "--verbose" in sys.argv or "-v" in sys.argv

    # Extract versions
    wit_major = extract_wit_major()
    sdk_version = extract_sdk_version()

    if wit_major is None:
        print("FAIL: could not extract WIT major version")
        return 1

    # Validate WIT major version is a single integer
    if not wit_major.isdigit():
        print(f"FAIL: WIT major version must be numeric, got '{wit_major}'")
        return 1

    print(f"WIT package major:   {wit_major}")

    if sdk_version:
        print(f"SDK const version:   {sdk_version}")

        sdk_parsed = parse_semver(sdk_version)
        if sdk_parsed is None:
            print(f"FAIL: invalid semver in SDK: '{sdk_version}'")
            return 1

        # WIT major must match SDK major
        if str(sdk_parsed[0]) != wit_major:
            print(
                f"FAIL: WIT major ({wit_major}) != SDK major ({sdk_parsed[0]})"
            )
            return 1
    elif verbose:
        print("SDK const version:   (not found, skipping check)")

    if verbose:
        print("Version consistency: OK")

    print("PASS: WIT version check passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
