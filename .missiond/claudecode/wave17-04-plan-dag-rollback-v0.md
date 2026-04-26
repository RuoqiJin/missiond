# Wave 17 Task 04 — PLAN DAG Rollback Policy v0

## Goal

Implement conservative rollback descriptors for failed DAG nodes.

This task does not need to automatically run destructive rollback actions. It should parse rollback policy, record rollback intent/evidence, and optionally dispatch rollback only when explicitly safe.

## Dependency

Run after Wave17-03 if both are active, because both touch `plan_dag.rs`.

## Ownership

Expected files:

- `crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs`
- `crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs` only if rollback can reuse task brief generation
- `crates/missiond-mcp/src/tools/knowledge/plan.rs`

Do not modify Lisp.

## Requirements

1. Parse rollback hints:

   - `:rollback-policy "none" | "descriptor" | "workstation"`
   - `:rollback-objective`
   - `:rollback-owned-files`
   - `:rollback-acceptance-commands`

2. Defaults:

   - no rollback if absent
   - no destructive action by default

3. Behavior:

   - `none`: preserve current failure behavior.
   - `descriptor`: return/write rollback descriptor; no dispatch.
   - `workstation`: dispatch rollback task only when all safety conditions hold:
     - target project resolved
     - objective non-empty
     - owned files present
     - dispatch strategy safe

4. SafeDescriptor refusals must not be retried.

5. Evidence:

   - rollback_policy
   - rollback_status: `not_requested | descriptor_ready | dispatched | refused | failed`
   - rollback task brief preview/path if generated

6. Failure-policy interaction:

   - rollback happens after final failed attempt, before downstream taint propagation.
   - downstream behavior remains governed by existing failure-policy.

## Tests

Add tests for:

- parser captures rollback policy
- descriptor mode does not dispatch
- workstation mode dispatches only with owned files/objective/project
- unsafe rollback returns refused descriptor
- downstream taint still follows failure-policy

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
git commit -m "feat(plan): record rollback policy for DAG failures"
```

Only stage files actually modified.

## Report

Return:

- Commit hash.
- Rollback policy contract.
- Safety refusals.
- Evidence shape.
- Tests and acceptance results.
