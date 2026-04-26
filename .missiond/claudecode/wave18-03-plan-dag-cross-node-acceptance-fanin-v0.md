# Wave 18 Task 03 — PLAN DAG Cross-Node Acceptance Fan-In v0

## Goal

Add conservative cross-node acceptance dependencies.

Wave17 added per-node acceptance modes. This task allows a node's acceptance to depend on evidence/status from prior nodes without arbitrary PLAN interpretation.

## Ownership

Expected files:

- `crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs`
- `crates/missiond-daemon/src/handlers/knowledge/evidence_collector.rs` only if needed
- `crates/missiond-mcp/src/tools/knowledge/plan.rs`

Do not modify Lisp in this task.

## Requirements

1. Parse optional node hints:

   - `:acceptance-depends-on ["node-a" "node-b"]`
   - `:acceptance-requires "all_succeeded" | "any_succeeded" | "evidence_keys"`
   - optional `:acceptance-source-node`

2. Defaults:

   - absent cross-node hints preserve Wave17 behavior

3. Behavior:

   - `all_succeeded`: accept only if listed nodes succeeded
   - `any_succeeded`: accept if any listed node succeeded
   - `evidence_keys`: read listed source node evidence keys from existing sidecar shape

4. Missing dependency node is structured parse/validation error.

5. Cycles remain handled by existing DAG dependency validation; acceptance dependency must not create new execution ordering cycles silently.

6. Evidence:

   - record fan-in source nodes
   - record fan-in result and reason

## Tests

Add tests for:

- parser captures acceptance-depends-on
- all_succeeded passes/fails
- any_succeeded passes/fails
- missing source node rejected
- absent hints preserve prior behavior

## Acceptance Commands

```bash
cargo test -p missiond-daemon handlers::knowledge::plan_dag::tests
cargo test -p missiond-daemon handlers::knowledge::evidence_collector::tests
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
        crates/missiond-daemon/src/handlers/knowledge/evidence_collector.rs \
        crates/missiond-mcp/src/tools/knowledge/plan.rs
git commit -m "feat(plan): support cross-node acceptance fan-in"
```

Only stage files actually modified.

## Report

Return:

- Commit hash.
- Fan-in contract.
- Evidence shape.
- Tests and acceptance results.
