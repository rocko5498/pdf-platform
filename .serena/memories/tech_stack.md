# Tech stack

- Rust edition 2021, MSRV 1.80 (`core/rust-toolchain.toml` channel stable)
- Cargo workspace only under `core/` (not repo root)
- Qt 6 Widgets planned for shell; cxx single FFI boundary (ADR-004)
- External oracle: qpdf process (not linked) for corpus-diff
- No async runtime in core (GR-6 / ADR-010)
- Deps: flag new crates.io; AGPL forbidden linked (ADR-028)
