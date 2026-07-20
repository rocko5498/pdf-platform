# Serena Rust LS

`.serena/project.yml` MUST set:
```yaml
languages:
  - rust
```
Empty `languages: []` → Active languages: [] → all symbol tools fail.

After changing languages, restart Serena MCP / Grok session so LS spawns.
rust-analyzer on PATH: `rustup component add rust-analyzer`.

codebase-memory-mcp often times out at Grok connect; raise MCP startup timeout if needed.
