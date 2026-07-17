# tools/ — harnesses only (not product runtime)

**Authority:** ADR-024 (repo layout), ADR-022/023 (test/bench), `docs/release-gates.md`.

Product runtime is **Rust core + Qt shell** only (ADR-002, ADR-003). Nothing under
`tools/` is linked into the application.

## Doc-listed CI gates (release-gates.md)

| Command | Language | Purpose |
|---|---|---|
| `python tools/a11y_audit.py` | Python | Static a11y label/role regression gate for `shell/` |
| `python tools/bench/check_p95_gates.py` | Python | Validate `bench/p95_gates.toml` is well-formed |

These two scripts are the **only** Python entry points named in the canonical
docs. They are CI/dev helpers; they do not parse PDF content or ship to users.

## Rust tooling

| Path | Purpose |
|---|---|
| `corpus-diff/` | Workspace package — render/structure corpus gate (`cargo run -p corpus-diff`) |
| `bench/` | Gate table + notes; real benches live in `core/benchmarks` |

## Policy

- Do **not** add a Python runtime dependency to `core/`, `shell/`, or the worker.
- Do **not** put document interpretation or product logic in Python.
- Prefer new harnesses as Cargo packages under `tools/` or `core/` when they
  exercise the real stack; keep Python limited to trivial file/TOML/regex gates
  already listed in release-gates, unless a superseding ADR says otherwise.

Plugin authoring in Python is a **future WASM/WIT binding** possibility (ADR-015),
not part of the application binary.
