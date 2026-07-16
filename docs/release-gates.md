# Release Gates Checklist

**Cites:** ADR-022, ADR-023, ADR-029, SDS §14, NFR-A11Y

Run before tagging a release. Do **not** invent numbers — attach real outputs.

## Automated (CI / local)

```bash
cd core
cargo test --workspace
cargo run -q -p corpus-diff
cargo test -p pdf-model --test interop_matrix
cargo test -p pdf-model forms_js
cargo test -p sandbox confinement
cargo test -p text-extract --test extraction_correctness
cd ..
python tools/a11y_audit.py
python tools/bench/check_p95_gates.py
```

## Manual / lab

- [ ] Screen reader pass (`docs/a11y-audit-checklist.md`)  
- [ ] p95 benches on **reference hardware** vs `tools/bench/p95_gates.toml`  
- [ ] Optional: Acrobat/Foxit open of XFDF round-trip sample  
- [ ] Confinement remains Advisory until `docs/security/confinement-review-package.md` signed  

## Assembly (needs qpdf)

```bash
cd core
cargo run -p cli --bin pdf-platform -- merge a.pdf b.pdf -o out.pdf
cargo run -p cli --bin pdf-platform -- split in.pdf -o split-out/
cargo run -p cli --bin pdf-platform -- extract-pages in.pdf 1 1 -o page1.pdf
cargo run -p cli --bin pdf-platform -- optimize in.pdf -o out.pdf screen
cargo test -p pdf-model assembly_ops
```

`qpdf` must be on `PATH`. Preflight without mutation: `optimize-preflight`.
