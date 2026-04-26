# Wave 17 Task 05 — PLAN DAG Finalize + Distill Trigger v0

## Goal

Finalize plan status after DAG execution and optionally trigger workflow distillation.

Current PLAN DAG execution records node outcomes, but full mark-final / distill linkage remains pending. This task implements a conservative v0.

## Dependency

Run after Wave17-04 if both are active, because both touch plan execution.

## Ownership

Expected files:

- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`
- `crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs`
- `crates/missiond-daemon/src/handlers/knowledge/workflow.rs`
- `crates/missiond-mcp/src/tools/knowledge/plan.rs`

Do not modify Lisp.

## Requirements

1. Add execute args:

   - `finalize_plan` bool, default false for compatibility
   - `distill_on_success` bool, default false
   - optional `distill_mode`, default `dry_run`

2. Finalization:

   - all terminal and no failed nodes -> plan status `succeeded`
   - any failed node -> plan status `failed` or existing equivalent status
   - any paused/manual_required node -> plan remains `executing` or `awaiting_review`; do not lie

3. Distill trigger:

   - only after successful finalization
   - call existing `mission_workflow(action=distill)` path
   - default dry-run unless caller explicitly sets `distill_mode="sonnet"`
   - distill failure surfaces warning/partial, does not corrupt plan final state

4. Evidence:

   - record final aggregate status
   - record distill trigger result or skipped reason

5. Backward compatibility:

   - Without `finalize_plan`, existing response/status behavior remains.

## Tests

Add tests for:

- successful DAG finalizes plan succeeded
- failed node finalizes failed
- paused node does not claim success
- distill_on_success dry-run calls/returns descriptor
- distill failure surfaces warning but keeps final status
- finalize_plan absent keeps legacy behavior

## Acceptance Commands

```bash
cargo test -p missiond-daemon handlers::knowledge::plan_dag::tests
cargo test -p missiond-daemon handlers::knowledge::plan::tests
cargo test -p missiond-daemon handlers::knowledge::workflow::tests
cargo test -p missiond-daemon
cargo test -p missiond-mcp --lib
cargo build --workspace
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check
```

## Commit

```bash
git add crates/missiond-daemon/src/handlers/knowledge/plan.rs \
        crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs \
        crates/missiond-daemon/src/handlers/knowledge/workflow.rs \
        crates/missiond-mcp/src/tools/knowledge/plan.rs
git commit -m "feat(plan): finalize DAG execution and trigger distill"
```

Only stage files actually modified.

## Report

Return:

- Commit hash.
- Finalization rules.
- Distill trigger behavior.
- Tests and acceptance results.
