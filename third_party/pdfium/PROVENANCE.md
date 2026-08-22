# PDFium Provenance

**Origin:** https://pdfium.googlesource.com/pdfium  
**License:** BSD 3-Clause (see LICENSE file in upstream source)  
**Snapshot:** pinned in `provenance.toml` at ref `chromium/7690` (published 2026-02-16), with a
recorded SHA-256 per platform. That file is the machine-readable source of truth; this one is
the human-readable summary.  

## Prebuilt binaries

The shared library is installed into `prebuilt/<platform>/` by
`python tools/provision_engine.py`, which verifies the artifact against the SHA-256 in
`provenance.toml` and installs nothing on mismatch. The binaries are **not** committed —
SDS §13.4 fetches them with a setup step, so `prebuilt/` is gitignored and only the
manifest travels in git. CI runs the setup step before `cargo build`.

Nothing at runtime may fetch this library: Z1 has no network (GR-1) and the product
transmits nothing without an explicit user action (GR-9). `engine-pdfium` binds from
`PDFIUM_LIB_PATH`, then the executable's directory, then `prebuilt/<platform>/`, and
reports an actionable diagnostic if all three miss. Publisher risk and the
reproducibility gap are recorded in ADR-038.

## Patches

Local patches in `patches/` are applied in order. Each patch file is named
`NNNN-description.patch` and documents its motivation in the header.

The series is empty and `tools/provision_engine.py` applies no patches: it installs the
published artifact unmodified. Applying a patch series would require building PDFium from
source, which ADR-029 keeps out of the contributor path. Stated rather than implied, per
ADR-028 §3.
