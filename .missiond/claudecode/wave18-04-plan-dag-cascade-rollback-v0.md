# Wave 18 Task 04 — PLAN DAG Cascade Rollback v0

## Goal

Extend Wave17 rollback from node-local descriptor/dispatch to conservative cascade planning.

This does not run destructive rollback automatically by default. It computes which downstream/upstream compensation nodes should be considered and records the plan.

## Dependency

Run after Wave18-03 if both are active, because both touch `plan_dag.rs`.

## Ownership

Expected files:

- `crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs`
- `crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs` only if reusing rollback task brief helpers
- `crates/missiond-mcp/src/tools/knowledge/plan.rs`

Do not modify Lisp.

## Requirements

1. Parse optional hints:

   - `:compensates "node-id"`
   - `:rollback-cascade "none" | "plan" | "dispatch-safe"`
   - `:rollback-after ["node-a" "node-b"]`

2. Defaults:

   - preserve Wave17 node-local rollback behavior

3. Cascade plan:

   - identify failed node
   - identify compensation nodes whose `:compensates` matches it
   - order compensation nodes by dependencies / rollback-after
   - output descriptor even when not dispatched

4. `dispatch-safe`:

   - only dispatch compensation nodes when their safety gates pass
   - no prompt fallback
   - SafeDescriptor refusals recorded, not retried

5. Evidence:

   - cascade root node
   - compensation nodes
   - dispatch/refusal result per compensation node

6. No arbitrary rollback code execution.

## Tests

Add tests for:

- parser captures compensates / rollback-cascade
- cascade plan finds compensation node
- ordering respects rollback-after
- dispatch-safe refuses unsafe compensation
- default mode unchanged

## Acceptance Commands

```bash
cargo test -p missiond-daemon handlers::knowledge::plan_dag::tests
cargo test -p missiond-daemon handlers::knowledge::workstation_dispatch::tests
cargo test -p missiond-daemon handlers::knowledge::plan::tests
cargo test -p missiond-daemon
cargo test -p missiond-mcp --lib
cargo build --workspace
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check
```

## Commit

```bash
git add crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs \
        crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs \
        crates/missiond-mcp/src/tools/knowledge/plan.rs
git commit -m "feat(plan): plan cascade rollback for DAG failures"
```

Only stage files actually modified.

## Report

Return:

- Commit hash.
- Cascade rules.
- Safety behavior.
- Tests and acceptance results.
