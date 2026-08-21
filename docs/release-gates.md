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
cargo test -p text-extract --test extraction_correctness   # see note below
cd ..
python tools/a11y_audit.py
python tools/bench/check_p95_gates.py
```

### What `extraction_correctness` does and does not cover

The suite drives a `StaticEngine` that returns hand-written `PageTextModel`
values. **No PDF is parsed and no engine runs.** The ligature, soft-hyphen, CJK
and RTL cases are strings typed into the test, so what it proves is that
normalization and search behave correctly *given* a text model — real and worth
gating, but not extraction accuracy.

MET-FEAT-4 and T-2 measure extraction against the document corpus. Nothing in
this checklist discharges that yet: `tools/corpus-diff` carries two fixtures.
The suite's own header says as much ("Full multi-engine corpus remains a
separate gate with PDFium fixtures"); this note carries that qualifier into the
checklist a release decision is actually made from. [MET-FEAT-4, T-2, PRIN-6, GR-8]

The CI step is likewise named "Extraction correctness suite (M2)". Renaming it
is deliberately left out of this change to avoid a third concurrent editor of
`.github/workflows/ci.yml`.

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

1. **Green CI on the release commit** (workspace tests, corpus-diff, interop, forms_js, extraction_correctness, a11y static, p95 table present). Read the two notes below before treating any of these as discharging a metric.
2. **Manual lab** items above that apply to the claim (viewer a11y for M1-tagged RC; XFDF open for M4-tagged RC).
3. **Confinement claim language:** only “advisory” until the review package is signed — never market as “fully sandboxed.”
4. **Tag format** (when a human cuts a tag): `v0.<milestone>.0-rc.N` e.g. `v0.3.0-rc.1` for M3-aligned RC. Prefer annotated tags after checklist sign-off.
5. **Changelog body** must cite FR/ADR/SDS IDs for shipped behavior and list honest non-claims.

Agents may prepare docs and verify gates; **humans** cut public tags and GitHub Releases.
