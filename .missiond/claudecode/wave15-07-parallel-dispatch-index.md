# Wave 15 Task 07 — Parallel Dispatch Index

This is not an implementation task. It is the recommended dispatch order for Wave15.

## Phase A — Serial Cleanup

Run first:

1. `wave15-00-archive-wave14-task-docs.md`
2. `wave15-01-fix-event-dispatcher-domain-count.md`

Reason:

- Task 00 keeps the worktree readable.
- Task 01 fixes a known red integration test before larger changes.

## Phase B — L2 Shard Lane

Run after Phase A:

3. `wave15-02-l2-shard-split-execution.md`
4. `wave15-03-shard-aware-source-checker.md`

Rules:

- Task 02 should run alone among Lisp-writing tasks.
- Task 03 starts only after Task 02 commits.
- Keep the resident Lisp session for Task 02.

## Phase C — Code Alignment Lane

Can run after Phase A. Prefer after Phase B if you want minimum conflict with checker/index work.

5. `wave15-04-review-gate-resolution-resume-v0.md`
6. `wave15-05-autonomous-workstation-dispatch-v0.md`

Parallelism:

- These two may run in parallel if their agents respect file ownership.
- If both need `plan.rs`, let Task 04 own review-gate resume surfaces first, then Task 05 rebase/read latest before editing.

ClaudeCode hint:

```text
使用 agent-team提高效率
```

Use this hint only for broad scan/refactor work where file ownership is clear.

## Phase D — Status Backfill

Run last:

7. `wave15-06-lisp-backfill-wave15-status.md`

Reason:

- It should reflect committed truth, not in-flight claims.

## Commit Policy

For Wave15, each task should create a scoped commit after its own acceptance checks pass.

Each commit must contain only the files owned by that task.

Do not use a shared stash during parallel work unless necessary. If a stash is necessary, report:

- stash name/hash
- files stashed
- files restored
- whether any conflict marker appeared

## Global Acceptance After Wave15

After all completed tasks:

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
