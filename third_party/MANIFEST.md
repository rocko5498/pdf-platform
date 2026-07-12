# Third-Party Dependency Manifest

All vendored or bundled third-party code must be recorded here per DEP-1.
New entries require: name, version, license, exit-seam description.

| Name    | Version | License      | Used by               | Exit seam |
|---------|---------|------------- |-----------------------|-----------|
| toml11  | 4.x     | MIT          | shell/chrome (C++)    | Replace with another TOML C++ lib; only `chrome/` calls it. |
| PDFium  | pinned  | BSD 3-Clause | engine-pdfium (Rust)  | Swap engine-api impl; `engine-hayro` is the alternative. |

<!-- Add rows as new third-party code is introduced. Never remove rows — mark deprecated. -->
