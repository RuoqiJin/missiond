# Wave 17 Task 10 — Parallel Dispatch Index

This is the recommended dispatch order for Wave17.

## Phase A — Bookkeeping

Run first:

1. `wave17-00-archive-wave16-task-docs.md`

## Phase B — PLAN DAG Runtime Lane

Run serially. These tasks all touch `plan_dag.rs`.

2. `wave17-01-plan-dag-paused-resume-hook-v0.md`
3. `wave17-02-plan-dag-claim-lease-v0.md`
4. `wave17-03-plan-dag-acceptance-evaluator-v0.md`
5. `wave17-04-plan-dag-rollback-v0.md`
6. `wave17-05-plan-dag-finalize-and-distill-trigger-v0.md`

Use `使用 agent-team提高效率` only if the task agent splits read-only exploration internally and keeps the write set under its ownership.

## Phase C — Evidence / Workstation Lane

Can run after Phase A. Avoid overlap with Phase B if the task touches `plan_dag.rs`.

7. `wave17-06-evidence-event-log-query-v0.md`
8. `wave17-07-workstation-scoped-commit-default-v1.md`

Suggested:

- Task 07 can run parallel with Phase B after reading latest workstation code.
- Task 06 should wait if Phase B is actively changing plan_dag evidence calls.

## Phase D — Smoke + Lisp Backfill

Run last:

9. `wave17-08-unified-entry-paused-resume-e2e-smoke.md`
10. `wave17-09-lisp-backfill-wave17-status.md`

## Explicitly Postponed

Frontend Lisp and frontend rebuild remain postponed.

Reason: MissionD runtime loop still has backend closure tasks. UI should project a stable protocol, not chase moving state.

## Commit Policy

Each task should commit its own scoped result after acceptance checks pass.

Do not use shared stash unless necessary. If stash is used, report:

- stash hash/name
- files stashed
- files restored
- conflicts observed

## Global Acceptance After Wave17

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
