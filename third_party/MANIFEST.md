# Third-Party Dependency Manifest

All vendored or bundled third-party code must be recorded here per DEP-1.
New entries require: name, version, license, exit-seam description.

| Name    | Version | License      | Used by               | Exit seam |
|---------|---------|------------- |-----------------------|-----------|
| toml11  | v4.4.0  | MIT          | shell/chrome (C++)    | Vendored single header (`toml11/toml.hpp`, SHA-256 in `toml11/provenance.toml`). Replace with another TOML C++ lib; only `chrome/registry.cc` includes it. |
| PDFium  | chromium/7690 | BSD 3-Clause | engine-pdfium (Rust) | Swap the `engine-api` implementation; no alternative backend is currently present. Artifact, URLs and per-platform SHA-256 are pinned in `pdfium/provenance.toml`; installed by `tools/provision_engine.py`. Publisher risk recorded in ADR-038. |

| Tesseract | 5.x (CI: distro/brew/choco) | Apache-2.0 | ocr-bridge (Rust) | Invoked as an external `tesseract` binary discovered on PATH, never linked. Swap the `OcrEngine` implementation; nothing else calls it. Not vendored — installed as a build/test-environment dependency, so no bytes ship in this repo. |

<!-- Add rows as new third-party code is introduced. Never remove rows — mark deprecated. -->
