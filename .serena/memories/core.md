# pdf-platform core

Open-source multi-process PDF platform (Rust core + Qt shell). GPLv3. Offline-first.

## Layout
- `core/` — single Cargo workspace (ADR-025). Entry: `core/Cargo.toml`.
- `tools/corpus-diff` — workspace member via `../tools/corpus-diff`.
- `shell/` — Qt CMake skeleton (not wired).
- `docs/` — ADR/SDS/PRD authoritative. Precedence: ADR → SDS → PRD → UI DS → IG.

## M0 spine
CLI inspect path live. Corpus-diff+CI on main. Next: WorkerTransport → spawn → shmem/tile → bridge.

## Read next
- Build/commands: `mem:suggested_commands`
- Stack pins: `mem:tech_stack`
- Agent rules: `mem:conventions`
- Done checks: `mem:task_completion`
- Tooling: `mem:tooling/serena_rust` (languages must include rust)
