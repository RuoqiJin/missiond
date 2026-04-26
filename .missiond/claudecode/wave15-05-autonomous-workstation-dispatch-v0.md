# Wave 15 Task 05 — Autonomous Workstation Dispatch v0

## Goal

Implement a conservative v0 of autonomous workstation dispatch for plan-runner / PLAN DAG nodes.

The outcome should be: when a plan node clearly targets a ClaudeCode workstation task, MissionD can produce a scoped task brief and dispatch through existing MissionD substrates, preferring spawned/reused workstations over `claude -p`.

Do not implement broad private scheduling. Keep this bounded to explicit PLAN hints.

## Ownership

Expected files:

- `crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs`
- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`
- `crates/missiond-daemon/src/handlers/compute/task_delegate.rs` only if needed for passing through task-md metadata
- `crates/missiond-mcp/src/tools/knowledge/plan.rs`

Optional new module if it keeps the code cleaner:

- `crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs`

Do not modify Lisp in this task. Lisp status backfill is Wave15-06.

## Architecture Rules

1. Prefer `mission_task_delegate` / existing workstation substrate. Do not shell out to `claude -p`.

2. Treat `agent-team` as task text hint, not a new transport.

   Required literal for agent-team strategy:

   ```text
   使用 agent-team提高效率
   ```

3. Respect project-root resolution. Do not join relative cwd against process cwd.

4. Respect scoped commit handoff:

   - Include task scope in generated task brief.
   - Include commit requirement for code tasks.
   - Include "do not stage or commit outside owned files".

5. The feature must be opt-in through explicit PLAN hints or execute args.

6. If dispatch cannot be performed safely, return a structured descriptor instead of silently falling back to prompt mode.

## Suggested Input/Hint Contract

Support conservative hints already parsed by plan-runner:

- `:target "mission_task_delegate"`
- `:dispatch-strategy "fresh-code-alignment" | "resident-lisp" | "agent-team" | "mixed"`
- `:target-project`
- `:requested-cwd`
- `:objective`
- `:scope`
- `:owned-files`
- `:commit-policy "scoped"`

Unknown hint fields must be preserved in `node_hint_summary.unsupported_fields`; do not reinterpret arbitrary Lisp.

## Behavior

For eligible node:

1. Build task brief text with:

   - objective
   - owned files
   - forbidden files
   - acceptance commands
   - commit policy
   - agent-team hint when strategy is `agent-team`

2. Dispatch using existing internal handler path.

3. Record typed evidence through the Wave12/Wave13 evidence collector path.

4. Return response fields:

   - `workstation_dispatch_status`
   - `dispatch_strategy`
   - `task_brief_preview` or `task_brief_path`
   - `inner_result` when dispatched

## Tests

Add tests for:

- agent-team hint injection exactly once
- fresh-code-alignment task brief includes scoped commit policy
- resident-lisp strategy does not choose prompt mode
- missing project root returns structured safe descriptor
- unsupported hint field preserved
- dry_run produces no dispatch/evidence write

## Acceptance Commands

Run:

```bash
cargo test -p missiond-daemon handlers::knowledge::plan_dag::tests
cargo test -p missiond-daemon handlers::knowledge::plan::tests
cargo test -p missiond-daemon
cargo test -p missiond-mcp --lib
cargo build --workspace
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check
```

## Commit

After acceptance:

```bash
git add crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs \
        crates/missiond-daemon/src/handlers/knowledge/plan.rs \
        crates/missiond-daemon/src/handlers/compute/task_delegate.rs \
        crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs \
        crates/missiond-mcp/src/tools/knowledge/plan.rs
git commit -m "feat(plan): dispatch workstation tasks from plan nodes"
```

Only stage files actually modified.

## Report

Return:

- Commit hash.
- Final hint contract.
- Transport used.
- Exact non-goals kept.
- Tests and full acceptance results.
