# corpus-diff

M0 structural differential-testing gate. It compares the platform's structural
inspection result with qpdf for every fixture and fails when the results disagree.

Run from `core/`:

```bash
cargo run -q -p corpus-diff
```

The command requires qpdf on `PATH` and is enforced by `.github/workflows/ci.yml`.
Render-tile comparison is not implemented and must not be claimed by this harness.

Cites: ADR-022, SDS §14 M0 exit criteria.
