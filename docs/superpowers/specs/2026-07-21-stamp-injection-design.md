# Stamp Injection Design

**Citations:** FR-BATCH-1, FR-CLI-1, ADR-012, ADR-013, SDS §3.3, SDS §14 M6

## Problem

The CLI reports a stamp output without writing it, while batch watermark and Bates steps copy the input unchanged and report success. This violates the product's honesty and CLI-parity requirements.

## Decision

Add one stamp operation to `DocumentCoordinator`. It reads the page tree through the existing worker session, creates one standard Type1 font object and one content-stream object per page, patches each page's `/Contents` and font resources, and applies the entire edit as one undoable `CommandGroup`. The existing incremental writer produces the output. CLI and batch entry points call this shared path.

Only classic page dictionaries and resource shapes that can be patched without ambiguity are accepted. Unsupported nested or indirect resource layouts return a clear error and never produce a false-success copy.

## Alternatives

- Calling qpdf overlay was rejected because it bypasses the coordinator's single-writer command path.
- Keeping stream generation only was rejected because it does not satisfy the documented CLI behavior.
- Building a new parser was rejected; existing worker object access and page-tree helpers are reused.

## Data flow

1. CLI or batch opens `DocumentCoordinator`.
2. Coordinator resolves page object numbers and dimensions.
3. `pdf-model` builds page patches plus new font/content objects.
4. A single command group updates the CoW overlay.
5. `save_incremental` writes a non-destructive incremental revision.

## Verification

- Unit tests cover `/Contents` missing, indirect, and array forms plus resource injection and rejection.
- A coordinator integration test stamps the fixture, saves it, and verifies the output contains stamp text and new references.
- CLI and batch tests verify a changed PDF is produced and false-success copy behavior is gone.
