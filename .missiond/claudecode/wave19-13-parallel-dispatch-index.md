# Wave 19 Parallel Dispatch Index

Wave 19 uses Lisp task contracts as the SSOT:

- Source contracts: `.missiond/tasks/wave19/wave19-*.lisp`
- Rendered ClaudeCode briefs: `.missiond/claudecode/wave19-*.md`
- Contract checker: `node scripts/check-task-contract.mjs --all`

Every implementation task requires its own scoped commit. Do not batch unrelated
tasks into one commit unless the task contract explicitly says so.

## Group 0 — Baseline

Can run together:

- `wave19-00-machine-contract-pilot`
  - Lisp: `.missiond/tasks/wave19/wave19-00-machine-contract-pilot.lisp`
  - Brief: `.missiond/claudecode/wave19-00-machine-contract-pilot.md`
- `wave19-01-archive-wave18-task-docs`
  - Lisp: `.missiond/tasks/wave19/wave19-01-archive-wave18-task-docs.lisp`
  - Brief: `.missiond/claudecode/wave19-01-archive-wave18-task-docs.md`

## Group 1 — Machine Contract Infrastructure

Can run in parallel after Group 0:

- `wave19-02-task-contract-verifier-v1`
- `wave19-03-report-contract-v1`
- `wave19-04-shared-memory-ledger-v0`

These touch separate scripts/schema files. If any agent needs to edit
`scripts/lib/missiond_lisp.mjs`, coordinate through the shared-memory ledger
once task 04 lands.

## Group 2 — Renderer

Run after Group 1:

- `wave19-05-renderer-dispatch-brief-v1`

This touches `scripts/render-claudecode-task.mjs` and re-renders the pilot.

## Group 3 — Independent Runtime Closures

Can run while Group 1/2 is in progress, because write scopes are disjoint:

- `wave19-09-cross-plan-distill-auto-chain-v1`
  - touches `workflow.rs` and MCP workflow schema
- `wave19-10-plan-dag-forward-compensate-ref-v0`
  - touches `plan_dag.rs` only

Do not run task 10 at the same time as task 06 if task 06 chooses to edit
`plan_dag.rs`.

## Group 4 — Task Contract Runtime Integration

Run after Group 2. These can mostly run together, with the conflict notes below:

- `wave19-06-plan-task-contract-emitter-v0`
  - touches `plan.rs`, `plan_dag.rs`, MCP plan schema
- `wave19-07-workstation-task-contract-consumer-v0`
  - touches `workstation_dispatch.rs`
- `wave19-08-execution-task-contract-completion-v0`
  - touches `agent_execution.rs`, MCP agent_execution schema

Conflict rule:

- Do not run task 06 with task 10 unless task 10 is already committed.
- Do not run task 08 with task 11.

## Group 5 — Execution Event Completion

Run after task 08:

- `wave19-11-execution-opened-dispatch-metadata-v1`

It touches `execution.rs` and `agent_execution.rs`.

## Group 6 — Lisp Backfill

Run last, after all committed code tasks:

- `wave19-12-lisp-backfill-wave19-status`

Use the resident Lisp architect session. Backfill only committed facts.
