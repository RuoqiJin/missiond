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

## Project Discovery

When a user mentions a project, service, domain, URL, repo name, or product alias and no explicit MissionD project ID is already known, call `mission_project` with `action="resolve"` first. Use `matched_project_id` from a `resolved` result for KB, Board, conversation, SSOT, and worker-dispatch calls. Do not conclude that a project is absent from `mission_project list` alone; `resolve` also checks compiled project universe aliases and service-runtime domains/URLs, and returns `unregistered_candidate` with a registration proposal when the target is not registered yet.
