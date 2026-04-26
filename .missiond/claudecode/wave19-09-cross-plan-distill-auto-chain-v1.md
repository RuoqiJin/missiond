# wave19-09-cross-plan-distill-auto-chain-v1 — Cross-plan distill auto-chain v1

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave19/wave19-09-cross-plan-distill-auto-chain-v1.lisp`

## Machine Contract

- kind: `code-alignment`
- status: `ready`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`

## Goal

Remove the explicit chain_id requirement for safe cross-plan distill chain recording by deriving a deterministic chain id from plan/workflow context.

## Ownership

- `crates/missiond-daemon/src/handlers/knowledge/workflow.rs`
- `crates/missiond-mcp/src/tools/knowledge/workflow.rs`

## Must Not Touch

- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`
- `crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs`
- `crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs`
- `.missiond/v2/*.lisp`
- `scripts/**`

## Requirements

1. Add an opt-in auto_chain mode for cross-plan distill chain recording; default must preserve current explicit behavior.
2. Derive deterministic chain_id from project root, plan_id, workflow name/id, and source evidence hash where available.
3. Never call Sonnet implicitly; keep sonnet only explicit.
4. Persist sidecar append-only; do not add migrations.

## Acceptance Commands

```bash
cargo test -p missiond-daemon handlers::knowledge::workflow::tests
cargo test -p missiond-daemon
cargo test -p missiond-mcp --lib
cargo build --workspace
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check -- crates/missiond-daemon/src/handlers/knowledge/workflow.rs crates/missiond-mcp/src/tools/knowledge/workflow.rs
```

## Commit

After acceptance, commit only files inside the declared write scope.

```bash
git add "crates/missiond-daemon/src/handlers/knowledge/workflow.rs" \
        "crates/missiond-mcp/src/tools/knowledge/workflow.rs"
git commit -m "feat(workflow): derive cross-plan distill chain ids"
```

Scope check: `write-scope-only`.

## Report

- `Commit hash.`
- `Derived chain id inputs.`
- `Backward compatibility notes.`
- `Acceptance command results.`

