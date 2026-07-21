# Third-Party Dependency Manifest

All vendored or bundled third-party code must be recorded here per DEP-1.
New entries require: name, version, license, exit-seam description.

| Name    | Version | License      | Used by               | Exit seam |
|---------|---------|------------- |-----------------------|-----------|
| toml11  | 4.x     | MIT          | shell/chrome (C++)    | Replace with another TOML C++ lib; only `chrome/` calls it. |
| PDFium  | unpinned | BSD 3-Clause | engine-pdfium (Rust) | Swap the `engine-api` implementation; no alternative backend is currently present. Exact snapshot remains blocked by `pdfium/PROVENANCE.md`. |

<!-- Add rows as new third-party code is introduced. Never remove rows — mark deprecated. -->
