# MissionD

Local-first orchestration layer for Claude Code & MCP Agents.

## Dev Setup

```bash
cargo build                    # Build all crates
cargo test -p missiond-core    # Run core tests
```

## Architecture

Current authority starts at `.missiond/v3/missiond-blueprint.lisp`, with project-local blueprints registered from V3 and compiled projections under `.missiond/v3/runtime/compiled/`. The legacy `.missiond/intent.lisp` files are historical/project compatibility inputs, not the MissionD control-plane SSOT.

For the operational read path, start with `docs/MISSIOND_OPERATOR_MANUAL.md`, then use `docs/MODULE_CATALOG.md` for module references. Before relying on a Lisp/code contract, run `node scripts/check-v3-final-convergence.mjs --json --static-only`.
