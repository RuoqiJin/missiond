# Wave 22 Parallel Dispatch Index

Wave 22 promotes Wave21 proposal paths into explicit apply gates. The `.md`
brief is only the ClaudeCode view; `.missiond/tasks/wave22/*.lisp` remains the
SSOT.

Shared memory:

- `.missiond/tasks/wave22/shared-memory.lisp`

Each worker should append `claim`, then `completion` / `blocker`, and run the
task-run verifier where the brief requires it.

## Group 0 — Archive

Run first:

- `wave22-00-archive-wave21-task-artifacts`

This commits Wave21 task contracts, rendered briefs, reports, and shared memory.

## Group 1 — Guard / Verification

Can run after Group 0:

- `wave22-01-hooks-default-on-doctor-v2`
- `wave22-02-execution-auto-run-verifier-v2`

These touch separate areas: scripts/render/schema vs `agent_execution.rs`.

## Group 2 — Apply Gates

These are substantial and may conflict through `plan.rs`; serialize if needed:

- `wave22-03-review-llm-approve-apply-gate-v1`
  - touches `review_gate.rs`, `directive.rs`, `plan.rs`, MCP directive/plan
- `wave22-04-persisted-plan-inference-apply-v2`
  - touches `plan.rs`, `plan_dag.rs`, MCP plan
- `wave22-05-autonomous-workstation-true-spawn-v1`
  - touches `workstation_dispatch.rs`, `plan.rs`, MCP plan

Recommended order: 03, then 04, then 05 unless agents explicitly coordinate
non-overlapping helper ownership.

## Group 3 — Workflow Policy

Can run while Group 2 is in progress if `plan.rs` conflicts are controlled:

- `wave22-06-distill-chain-policy-auto-sonnet-v2`

Touches `workflow.rs`, `plan.rs`, MCP workflow. If Group 2 is actively editing
`plan.rs`, wait.

## Group 4 — E2E Smoke

Run after Groups 1-3:

- `wave22-07-autonomous-loop-apply-smoke-v4`

Touches `unified_entry.rs`, `plan.rs`, `workstation_dispatch.rs`,
`agent_execution.rs`, and `review_gate.rs`. Run late.

## Group 5 — Lisp Backfill

Run last:

- `wave22-08-lisp-backfill-wave22-status`

Use the resident Lisp architect session. Backfill only committed facts and keep
frontend Lisp postponed.

## Suggested Operator Order

1. Send `wave22-00`.
2. Send `wave22-01` and `wave22-02` in parallel.
3. Send `wave22-03`, `wave22-04`, `wave22-05` serially unless clear ownership is split.
4. Send `wave22-06`.
5. Send `wave22-07`.
6. Send `wave22-08`.
