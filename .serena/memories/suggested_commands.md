# Commands (Windows PowerShell)

```
cd core
cargo build --workspace
cargo test --workspace
cargo build -p cli
cargo run -p cli --bin pdf-platform -- <file.pdf>
# qpdf must be on PATH for corpus-diff:
cargo run -p corpus-diff
cargo test -p protocol -p sandbox
```

Git: use `rtk` prefix if hook rewrites bash; PowerShell may need full PATH for gh/cargo.
