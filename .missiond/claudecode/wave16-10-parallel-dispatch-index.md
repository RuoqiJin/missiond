# Wave 16 Task 10 — Parallel Dispatch Index

This is the recommended dispatch order for Wave16.

## Phase A — Bookkeeping

Run first:

1. `wave16-00-archive-wave15-task-docs.md`

## Phase B — Review Gate Lane

Run serially:

2. `wave16-01-workflow-review-resolution-v0.md`
3. `wave16-02-review-gate-question-listener-v0.md`

Reason:

- Task 02 depends on workflow resolution behavior from Task 01.
- Both may touch `review_gate.rs`.

## Phase C — Plan / Workstation Lane

Run serially because all touch `plan_dag.rs`:

4. `wave16-03-workstation-dispatch-auto-inference-v1.md`
5. `wave16-04-plan-dag-review-pause-v0.md`
6. `wave16-05-plan-dag-retry-policy-v0.md`

ClaudeCode hint for broad scan/refactor:

```text
使用 agent-team提高效率
```

Use it only when file ownership is clear.

## Phase D — Independent Execution/Evidence Lane

Can run after Phase A. Avoid overlapping with Phase C if it needs `plan_dag.rs`.

7. `wave16-06-scoped-commit-enforce-v0.md`
8. `wave16-07-evidence-event-subscription-v0.md`

Suggested:

- Task 06 can run in parallel with Phase B/C.
- Task 07 should wait if Phase C is actively editing `plan_dag.rs`.

## Phase E — Integration and Lisp

Run after chosen implementation tasks have committed:

9. `wave16-08-unified-entry-e2e-smoke-v0.md`
10. `wave16-09-lisp-backfill-wave16-status.md`

## Commit Policy

Each task should commit its own scoped result after acceptance checks pass.

Do not use shared stash unless necessary. If stash is used, report:

- stash hash/name
- files stashed
- files restored
- conflicts observed

## Global Acceptance After Wave16

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
