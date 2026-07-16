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

## Release candidate (RC) procedure

Do **not** invent pass/fail numbers. Attach real CI logs and bench outputs.

1. **Green CI on the release commit** (workspace tests, corpus-diff, interop, forms_js, extraction_correctness, a11y static, p95 table present).
2. **Manual lab** items above that apply to the claim (viewer a11y for M1-tagged RC; XFDF open for M4-tagged RC).
3. **Confinement claim language:** only “advisory” until the review package is signed — never market as “fully sandboxed.”
4. **Tag format** (when a human cuts a tag): `v0.<milestone>.0-rc.N` e.g. `v0.3.0-rc.1` for M3-aligned RC. Prefer annotated tags after checklist sign-off.
5. **Changelog body** must cite FR/ADR/SDS IDs for shipped behavior and list honest non-claims.

Agents may prepare docs and verify gates; **humans** cut public tags and GitHub Releases.
