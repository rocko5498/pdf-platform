# Release Gates Checklist

**Cites:** ADR-022, ADR-023, ADR-029, SDS §14, NFR-A11Y

Run before tagging a release. Do **not** invent numbers — attach real outputs.

## Automated (CI / local)

```bash
python tools/provision_engine.py   # installs the pinned PDFium; verifies SHA-256
cd core
cargo test --workspace
cargo run -q -p corpus-diff
cargo test -p pdf-model --test interop_matrix
cargo test -p pdf-model forms_js
cargo test -p sandbox confinement
cargo test -p text-extract --test extraction_correctness   # normalization only; see note
cargo test -p engine-pdfium --test extraction_accuracy     # real PDF through PDFium
cd ..
python tools/a11y_audit.py
python tools/bench/check_p95_gates.py
python tools/verapdf_writer_gate.py   # needs a JRE; writer output must parse
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

The CI step is now named "Text normalization suite (M2, no PDF parsed)", which
is what it does.

`extraction_accuracy` measures the thing MET-FEAT-4 names. Its fixtures carry
explicit `/ToUnicode` CMaps, so the expected text is fixed by the file rather
than by engine behaviour, and it covers the cases M2's exit criteria list:
Latin, ligature, soft hyphen, RTL, CJK, and a Private-Use mapping that must be
flagged unreliable.

It does **not** cover embedded font subsets, CID-keyed fonts or vertical
writing, and it says nothing about rendering fidelity — that is `corpus-diff`.
MET-FEAT-4 is substantially advanced, not discharged: the long tail is
real-world documents, which this corpus is not.

### The engine must be provisioned first

`cargo test --workspace` without a provisioned PDFium exercises the stub paths and proves
nothing about rendering. `python tools/provision_engine.py --check` reports whether the
pinned artifact is installed; CI runs the install step before `cargo build`.
[ADR-028, ADR-038, SDS §13.4]

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
