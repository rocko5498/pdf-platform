# Benchmark harness notes

**Cites:** ADR-023, PRD §14 MET-PERF-*, SDS §14 M1

## Gates

Budgets live in [`p95_gates.toml`](p95_gates.toml). Validate definition:

```bash
python tools/bench/check_p95_gates.py
```

## Run (developer)

```bash
cd core
cargo bench -p benchmarks --bench startup
cargo bench -p benchmarks --bench large_doc   # when registered
```

## CI policy

- PR CI: **definition smoke** only (`check_p95_gates.py`) — no noisy cloud gating.  
- Release: hard p95 compare on **fixed reference hardware** per ADR-023.
