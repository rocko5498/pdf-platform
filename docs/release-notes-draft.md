# Release notes draft: first stable candidate

**Status: draft, not a release.** Agents may prepare this file; a human cuts the tag
(`v0.<milestone>.0-rc.N`) after the checklist in [release-gates.md](release-gates.md) is signed.
Every claim below must be re-checked against the release commit's CI logs before use.
Per-criterion evidence: [milestone-exit-tracker.md](milestone-exit-tracker.md).

## Shipped since the 2026-10-10 baseline (merged PRs #87 to #104)

- **Opening real-world files** [FR-VIEW-2]: linearized files, PNG/TIFF-predictor cross-reference
  and object streams, compact incremental sections chained by `/Prev`, `startxref` beyond the
  last kilobyte, encrypted files whose page tree is in encrypted object streams (page count taken
  from the engine, disagreement disclosed).
- **Saving** [FR-SAVE]: incremental updates list what they wrote and chain with `/Prev`; the catalog
  need not be object 1; plaintext objects are refused into an encrypted document.
- **Page operations** (CLI) [FR-ORG-1, FR-CLI-1]: rotate (adds to inherited `/Rotate`), delete,
  crop, move, duplicate, insert blank. They read the whole page tree and edit nodes in place.
- **Revision history and sanitize** (CLI) [FR-VER-1, FR-VER-2].
- **Export** (CLI) [FR-EXPORT-1]: PNG per page, escaped HTML.
- **Printing** [FR-PRINT-1, FR-PRINT-2, FR-PRINT-3]: File > Print (Ctrl+P), native dialog.
- **Page thumbnails** [FR-THUMB-1]: lazy "Pages" dock.
- **Menu bar** built from the declared taxonomy [ADR-032].
- **Tagged flag** [FR-A11Y-1]: a tagged document now reports as structured.

## Non-claims (must stay in the final changelog) [ROAD-2, GR-8]

- Sandbox confinement is **advisory** until the review package is signed. Not "fully sandboxed".
- No hard p95 performance claim: no pinned reference hardware run (MET-PERF-*, MET-GOV-1).
- Screen-reader behaviour is not verified; the a11y audit is static.
- Reading order of tagged documents is **not** exposed or exported (FR-A11Y-1 exposure, FR-A11Y-3).
- Page operations, export, revisions and sanitize are CLI only; no GUI.
- Thumbnails cannot be dragged to reorder or rotate pages.
- Corpus is synthetic plus qpdf-generated shapes; no real-world corpus sweep.
- Save of encrypted documents is refused, not supported.
- Partial or version-2 scope, labelled as such: forms JavaScript, redaction (not implemented),
  signatures (validation only, never "Valid"), OCR (needs Tesseract), PDF/A (heuristic),
  plugins (stub), compare.
- Opening a PDF by hand in the built shell has not been done by a person.
