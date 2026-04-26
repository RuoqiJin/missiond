# Wave 16 Task 03 — Workstation Dispatch Auto-Inference v1

## Goal

Upgrade Wave15 workstation dispatch from explicit opt-in only to conservative auto-inference.

Current behavior requires `workstation_dispatch=true` or PLAN node `:workstation-dispatch true`. This task allows MissionD to infer workstation dispatch when a PLAN/DAG node is clearly a ClaudeCode workstation task.

No `claude -p`. No broad private scheduling. No arbitrary PLAN Lisp interpretation.

## Ownership

Expected files:

- `crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs`
- `crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs`
- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`
- `crates/missiond-mcp/src/tools/knowledge/plan.rs`

Do not modify Lisp; Wave16 backfill is later.

## Requirements

1. Auto-enable workstation dispatch only when all are true:

   - resolved target is `mission_task_delegate`
   - dispatch strategy is one of `fresh-code-alignment`, `resident-lisp`, `agent-team`, or `mixed`
   - objective is non-empty
   - at least one scoping signal exists: `owned_files`, `scope`, `target_project`, or `requested_cwd`
   - caller did not explicitly set `workstation_dispatch=false`

2. Preserve explicit behavior:

   - `workstation_dispatch=true` still forces the Wave15 path.
   - `workstation_dispatch=false` disables auto-inference.
   - Safety failures still return `SafeDescriptor`, never prompt fallback.

3. Add response fields:

   - `workstation_dispatch_source`: `explicit_arg | plan_hint | inferred | disabled | not_applicable`
   - `workstation_dispatch_inference_reason`

4. Agent-team hint remains exactly-once.

5. Do not auto-infer for `mission_execution` or `mission_flow_run`.

6. Do not auto-infer when target/project root is unresolved.

## Tests

Add tests for:

- inferred for mission_task_delegate + fresh-code-alignment + owned_files
- inferred for agent-team and literal injected exactly once
- not inferred when strategy unknown
- not inferred when objective missing
- explicit false disables inference
- explicit true preserves old behavior
- non-task-delegate targets not inferred

## Acceptance Commands

```bash
cargo test -p missiond-daemon handlers::knowledge::workstation_dispatch::tests
cargo test -p missiond-daemon handlers::knowledge::plan_dag::tests
cargo test -p missiond-daemon handlers::knowledge::plan::tests
cargo test -p missiond-daemon
cargo test -p missiond-mcp --lib
cargo build --workspace
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check
```

## Commit

```bash
git add crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs \
        crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs \
        crates/missiond-daemon/src/handlers/knowledge/plan.rs \
        crates/missiond-mcp/src/tools/knowledge/plan.rs
git commit -m "feat(plan): infer workstation dispatch for scoped task nodes"
```

## Report

Return:

- Commit hash.
- Final inference rule.
- Non-goals preserved.
- Tests and acceptance results.
