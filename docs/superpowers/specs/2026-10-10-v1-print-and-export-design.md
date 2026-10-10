# Version 1 output: printing and export

**Status:** part A (`export-images`) is implemented on branch `feat/v1-export-images`; `export-html` and part B (printing) are proposals only.
**Date:** 2026-10-10
**Cites:** FR-PRINT-1, FR-PRINT-2, FR-PRINT-3, FR-PRINT-5, FR-EXPORT-1, FR-EXPORT-3, FR-CLI-1, SDS §2.6, SDS §4.6, ADR-003, ADR-016, DS-FLOW-PRINT, GR-1, GR-3, GR-4, GR-7, PRIN-1, T-12

## Why

PRD §16.2 puts basic printing and export to images, text and HTML in version 1. A search of `core/` and `shell/` found neither printing (no `QPrinter` or `QPrintDialog`) nor image/HTML export. `export-text` is the only export command.

## What the documents already decide

- **Where printing lives.** SDS §2.6: preparation (scaling, page selection, later imposition) is a Z1 job; spooling to the OS is a Z0 broker operation using Qt's `QPrinter` and `QPrintDialog`. ADR-003 lists print integration in the shell, and the shell layout has a `dialogs/` directory for it.
- **How it looks.** DS-FLOW-PRINT: File ▸ Print (`Ctrl/Cmd+P`), the native dialog, with page range, scaling (fit, actual, custom), duplex, and a "print comments" toggle. Printed output must match the screen within the fidelity target.
- **Export is batch- and CLI-reachable.** FR-EXPORT-3, and `FR-CLI-1` lists `convert`.

## What exists (checked 2026-10-10)

- `DocumentCoordinator::render_page(page, scale)` (`core/coordinator/src/document.rs:733`) returns a page as RGBA through the worker; the scale is relative to 72 DPI.
- The transport frame cap is 16 MiB (`MAX_FRAME`, `core/protocol/src/transport.rs:13`), and the raster crosses it base64-encoded, so only three quarters of the frame is pixels. Measured on 2026-10-10: a 16.6 MB raster arrived as a 22,207,396-byte frame. A US-letter page fits up to 182 DPI (the largest the CLI accepts) and not at 300 DPI. The first draft of this note said the cap allowed 211 DPI; building the exporter showed that was wrong.
- A PNG writer exists only as a private function in `ocr-bridge` (`write_rgba_png`, `core/ocr-bridge/src/lib.rs:1011`).
- `export-text` already works from the page text model.

## Proposal

### A. Export, CLI first (no bridge change, no new dependency)

1. `export-images <file> -o <dir> [--dpi N] [--pages a-b]`: render each page with `render_page`, write PNG.
   - DPI above the single-frame limit needs tiling. V1 limits the default to 150 DPI and refuses a larger DPI with a message naming the limit, instead of producing a wrong image (GR-8). Tiled rendering at higher DPI is a later step.
   - PNG encoding: expose the existing writer from `ocr-bridge` as a `pub fn` rather than adding an image crate (AI-4). Its zlib header was fixed on 2026-08-22 and has a pinning test.
2. `export-html <file> -o <out.html>`: text from the same page text model that `export-text` uses, one `<section>` per page, reading order as extracted, with the reliability flag surfaced as a visible notice on unreliable pages (FR-SRCH-5 honesty). No positioned layout in V1; stated as a limit in the output header.
3. Batch: add `BatchStep` variants only if the parser stays strict about malformed steps (the 2026-08-23 fix).

Evidence needed (T-10, T-11, T-12): export a fixture, read the files back, check dimensions equal `page size × dpi / 72`, and that the PNG decodes to the same pixels the worker produced; for HTML, the known words appear in order. Each test is made to fail once.

### B. Printing

1. Shell: `shell/dialogs/print_dialog.{h,cc}` using `QPrintDialog`, opened from the File ▸ Print action already declared in `ui-registry.toml` (verify the id before use). Needs the Qt `PrintSupport` component in `shell/CMakeLists.txt`: a build change within Qt, which ADR-003 already adopts; flag for review (DEP-1).
2. Page images: for each selected page the shell requests tiles through the existing `render_tile` bridge call at the printer's resolution and draws them with `QPainter`. No new bridge call is needed for the first version, which keeps FFI-6 out of the first step. This is raster printing; text is not vector in the output. That is a stated V1 limit.
3. Options: range, fit/actual/custom scale, annotations toggle (draw or omit annotations in the tile request: needs verification of how annotations reach the tile today).
4. Keyboard operable and announced (FR-PRINT-5): `QPrintDialog` is native; add `shell.print_dialog` QTest for the option mapping.
5. Bounded memory (GR-7): print one page at a time and release the tile buffers.

Evidence needed: a QTest that prints to `QPrinter::PdfFormat` and checks the output PDF has the selected page count and a page that is not blank; made to fail by selecting the wrong range.

Not claimed in V1: vector output, imposition (FR-PRINT-4 is "when delivered"), driver-specific transparency handling, print preview.

## Open questions (resolve by reading, not by guessing)

- How annotations are drawn into tiles today: the tracker says the Qt canvas draws annotations itself, so printing them may need the canvas overlay code, not the worker.
- ~~Whether the registry already declares a Print action id.~~ Checked: `shell/chrome/ui-registry.toml` has no print entry. Declaring File ▸ Print and its shortcut changes `[shortcuts]` and `[[menus]]`, so ADR-032 requires a `profile_version` bump (now 0.2.0) under the ADR-030 review policy, and PR-11 treats it as an approved change, not an incidental one. The action id comes from the registry's own naming (`view.zoom_fit` style); pick it when writing the entry and let `tools/check-ui-registry` and `shell.menu_bar` verify it.
- ~~Whether `PrintSupport` is present in the installed Qt.~~ Checked: `Qt6PrintSupport.dll` and its CMake package exist in `C:\Qt\6.8.3\msvc2022_64`.

## Order of work

1. A (CLI export) first: no gated area, no new dependency beyond making one function public.
2. B (printing) second, as its own PR, because it changes the stability contract and the shell build.

## Review

Export (A) touches no human-gated area. Printing (B) adds a Qt component and a shell dialog but no bridge or `unsafe` code. Neither needs two reviewers unless the bridge changes.
