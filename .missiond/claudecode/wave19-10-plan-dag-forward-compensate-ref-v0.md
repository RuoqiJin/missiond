# wave19-10-plan-dag-forward-compensate-ref-v0 — PLAN DAG forward compensate ref v0

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave19/wave19-10-plan-dag-forward-compensate-ref-v0.lisp`

## Machine Contract

- kind: `code-alignment`
- status: `ready`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`

## Goal

Support forward :compensate-node references alongside existing reverse :compensates rollback semantics in the PLAN DAG parser and scheduler.

## Ownership

- `crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs`

## Must Not Touch

- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`
- `crates/missiond-daemon/src/handlers/knowledge/workflow.rs`
- `crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs`
- `.missiond/v2/*.lisp`
- `scripts/**`

## Requirements

1. Parse :compensate-node or :compensate-ref as a forward reference from a failing node to its compensation node.
2. Reject self references and unknown compensation nodes with structured parser errors.
3. If both forward and reverse declarations exist, require them to agree; do not silently choose one.
4. Keep rollback safety gates from Wave 18 intact.

## Acceptance Commands

```bash
cargo test -p missiond-daemon handlers::knowledge::plan_dag::tests
cargo test -p missiond-daemon
cargo build --workspace
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check -- crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs
```

## Commit

After acceptance, commit only files inside the declared write scope.

```bash
git add "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
git commit -m "feat(plan): support forward compensate node refs"
```

Scope check: `write-scope-only`.

## Report

- `Commit hash.`
- `Accepted forward-ref keys.`
- `Conflict and safety behavior.`
- `Acceptance command results.`

