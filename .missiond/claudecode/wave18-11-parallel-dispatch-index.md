# Wave 18 Task 11 — Parallel Dispatch Index

This is the recommended dispatch order for Wave18.

## Phase A — Bookkeeping

Run first:

1. `wave18-00-archive-wave17-task-docs.md`

## Phase B — Independent Infrastructure Lane

Can run after Phase A:

2. `wave18-01-event-log-query-api-v1.md`
3. `wave18-02-execution-event-dispatch-metadata-v1.md`
4. `wave18-08-scoped-commit-worktree-preflight-v0.md`

These should be mostly independent. Watch for evidence collector overlap between Task 01 and plan tasks.

## Phase C — PLAN DAG Lane

Run serially. These tasks touch `plan_dag.rs` / plan execution.

5. `wave18-03-plan-dag-cross-node-acceptance-fanin-v0.md`
6. `wave18-04-plan-dag-cascade-rollback-v0.md`
7. `wave18-05-cross-plan-distill-chain-v0.md`
8. `wave18-06-autonomous-plan-field-inference-v0.md`

Use `使用 agent-team提高效率` only for internal read-only exploration or clearly partitioned edits.

## Phase D — Review Automation Lane

Can run after Phase A, but avoid simultaneous edits to `plan.rs` if Phase C is active:

9. `wave18-07-review-automation-policy-v0.md`

## Phase E — Smoke + Lisp Backfill

Run last:

10. `wave18-09-unified-entry-autonomous-loop-smoke-v1.md`
11. `wave18-10-lisp-backfill-wave18-status.md`

## Explicitly Postponed

Frontend Lisp and frontend rebuild remain postponed.

Reason: the backend MissionD loop is still closing final automation gaps. The UI should project a stable protocol after the loop is reliable.

## Checker Note

The previous interactive checker call appeared to hang once, but a bounded rerun completed immediately:

```bash
/opt/local/bin/gtimeout 10s node scripts/check-architecture-lisp.mjs --all-v2
```

If a checker run hangs again, do not wait indefinitely. Kill after 30s, report it, and create a correction task for checker timeout/diagnostics.

## Commit Policy

Each task should commit its own scoped result after acceptance checks pass.

Do not use shared stash unless necessary. If stash is used, report:

- stash hash/name
- files stashed
- files restored
- conflicts observed

## Global Acceptance After Wave18

```bash
cargo test -p missiond-core --lib
cargo test -p missiond-core --test event_dispatcher_integration
cargo test -p missiond-daemon
cargo test -p missiond-mcp --lib
cargo build --workspace
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check
git status --short
```
