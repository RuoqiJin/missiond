# Wave 21 Parallel Dispatch Index

Wave 21 continues the machine-contract path. The rendered `.md` is an execution
view; the matching `.missiond/tasks/wave21/*.lisp` file is the SSOT.

Shared memory:

- `.missiond/tasks/wave21/shared-memory.lisp`

Each worker should append:

- `claim` before editing
- `observation` or `blocker` while running
- `completion` when committed

## Group 0 — Archive

Run first:

- `wave21-00-archive-wave20-task-artifacts`

This commits Wave20 task contracts, briefs, reports, and shared memory.

## Group 1 — Guardrail Infrastructure

Can run after Group 0:

- `wave21-01-hooks-path-installer-v1`
- `wave21-02-run-verifier-v1`

These are mostly scripts/schema work. They may both touch task schema docs, so
if either agent edits `.missiond/tasks/schema/task-contract-v1.lisp`, coordinate
via shared memory.

## Group 2 — Execution Verification

Run after Group 2 verifier lands:

- `wave21-03-execution-report-verifier-integration-v1`

Conflict: do not run with any other task touching `agent_execution.rs`,
especially `wave21-08`.

## Group 3 — LLM / Autonomous Proposals

Can run in parallel after Group 0, but avoid file conflicts:

- `wave21-04-autonomous-workstation-llm-proposal-v0`
  - touches `workstation_dispatch.rs`, `plan.rs`, MCP plan
- `wave21-06-llm-auto-approve-proposal-v0`
  - touches `review_gate.rs`, `directive.rs`, `plan.rs`, MCP directive/plan
- `wave21-07-sonnet-distill-chain-auto-apply-v1`
  - touches `workflow.rs`, `plan.rs`, MCP workflow

Conflict: all three may touch `plan.rs`; serialize those edits or make the
first landed commit the base for the next two.

## Group 4 — PLAN Inference Apply

Run after `wave21-04` if both touch plan inference paths:

- `wave21-05-plan-inference-apply-gate-v1`

Conflict: touches `plan.rs`, `plan_dag.rs`, MCP plan. Do not run together with
`wave21-04` or `wave21-06` unless the agents explicitly split helper ownership.

## Group 5 — E2E Smoke

Run after Groups 2-4:

- `wave21-08-machine-contract-autonomous-loop-smoke-v3`

This touches `unified_entry.rs`, `plan.rs`, `workstation_dispatch.rs`, and
`agent_execution.rs`, so it should run late.

## Group 6 — Lisp Backfill

Run last:

- `wave21-09-lisp-backfill-wave21-status`

Use the resident Lisp architect session. Backfill committed facts only; proposal
tasks stay proposal-only unless the code actually applies them.

## Suggested Operator Order

1. Send `wave21-00`.
2. Send `wave21-01` and `wave21-02`.
3. Send `wave21-03`.
4. Send `wave21-04`, then `wave21-05`.
5. Send `wave21-06` and `wave21-07` serially if `plan.rs` conflicts appear.
6. Send `wave21-08`.
7. Send `wave21-09`.
