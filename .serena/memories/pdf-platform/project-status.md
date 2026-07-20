# PDF Platform — Project Status

## Current milestone: M0-M7 complete (model + command layer)

## Test count: 232 passing (54 suites)

## Architecture
- Rust core (26 crates) + Qt Widgets shell (C++)
- Multi-process: coordinator (Z0) + sandboxed workers (Z1)
- CoW overlay + Command journal for undo/redo
- Engine-API traits (Rasterize, Extract, Structure) with PDFium backend

## Completed milestones
- M0: Walking skeleton + benchmarks
- M1: Render pipeline (Rust-side complete), compressed xref, PDFium structure queries
- M2: Text extraction + search
- M3: Mutation core (CoW, Commands, UndoJournal, incremental save)
- M4: Annotations (13 types, appearance streams, FDF/XFDF, threading)
- M5: Forms (AcroForm model, 7 field types)
- M6: Assembly (merge/split/optimize)
- M7: Redaction (provable, with verification)

## Remaining Rust-side work
- Batch job system (M6 completion)
- CLI subcommands (M6 completion)
- Diagnostics data assembly (M1 completion)
- M2 extraction correctness suite

## Remaining Qt-dependent work
- GPU tile compositor
- Scroll/zoom/rotate event forwarding
- Shell panels (outline, thumbnail, attachment, layer)
- Shell accessibility
- Password prompt dialog

## Next milestones
- M8: Signatures (PAdES validation + software signing)
- M9: OCR (Tesseract backend)
- M10: Hardware signing + compliance
- M11: Plugin system (WASM/WIT)
- M12: Content editing + compare
