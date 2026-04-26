# Wave 20 Parallel Dispatch Index

Wave 20 continues the Lisp-as-SSOT experiment. Send ClaudeCode the rendered
brief path, but treat the matching `.missiond/tasks/wave20/*.lisp` file as the
contract of record.

Shared memory:

- `.missiond/tasks/wave20/shared-memory.lisp`

Every worker should append a `claim` entry before editing and a `completion` or
`blocker` entry before handing back.

## Group 0 — Baseline Archive

Run first:

- `wave20-00-archive-wave19-task-contracts`
  - Brief: `.missiond/claudecode/wave20-00-archive-wave19-task-contracts.md`
  - Scope: Wave19 `.lisp` contracts + rendered `.md` briefs only

This cleans the untracked Wave19 artifacts before code work starts.

## Group 1 — Scope Guardrail

Run after Group 0:

- `wave20-01-task-scope-index-guard-v1`

This directly addresses the Wave19 staged-index pollution. Do not run renderer
changes before this lands.

## Group 2 — Renderer / Daemon Preflight

Can run in parallel after Group 1:

- `wave20-02-renderer-scoped-commit-guard-v2`
  - touches `scripts/render-claudecode-task.mjs`
- `wave20-03-execution-preflight-contract-scope-v1`
  - touches `agent_execution.rs` + MCP schema

Conflict: do not run task 03 with task 09, because both can touch
`agent_execution.rs`.

## Group 3 — Independent Runtime Closures

Can run while Group 2 is active:

- `wave20-06-cross-plan-distill-auto-trigger-v1`
  - touches `workflow.rs` + MCP workflow schema
- `wave20-08-review-auto-answer-policy-v0`
  - touches `review_gate.rs` + `unified_entry.rs`
- `wave20-09-execution-event-legacy-metadata-sweep`
  - touches `execution.rs` + `agent_execution.rs`

Conflict: task 08 touches `unified_entry.rs`; do not run it with task 05.

## Group 4 — Machine-Driven Dispatch

Run after Group 2:

- `wave20-04-machine-driven-dispatch-v0`

This is the main Wave20 architecture step: task.lisp becomes directly consumed
by workstation dispatch, while Markdown becomes compatibility output.

Potential conflicts:

- touches `plan.rs`, `plan_dag.rs`, `workstation_dispatch.rs`, `unified_entry.rs`
- do not run with task 05 or task 07
- if task 10 is still pending, wait until task 04 is committed before Lisp backfill

## Group 5 — Smoke / LLM Proposal

Run after task 04:

- `wave20-05-unified-entry-machine-loop-smoke-v2`
- `wave20-07-llm-augmented-plan-inference-v0`

Conflict: task 05 touches `unified_entry.rs`; task 07 touches `plan.rs` and
`plan_dag.rs`. They are disjoint enough if task 04 has already landed, but if
either needs to patch shared helpers introduced by task 04, serialize them.

## Group 6 — Lisp Backfill

Run last:

- `wave20-10-lisp-backfill-wave20-status`

Use the resident Lisp architect session. Backfill committed facts only.

## Suggested Operator Order

1. Send `wave20-00`.
2. Send `wave20-01`.
3. Send `wave20-02`, `wave20-03`, `wave20-06`, `wave20-08` in parallel.
4. Send `wave20-09` after `wave20-03`.
5. Send `wave20-04`.
6. Send `wave20-05` and `wave20-07`.
7. Send `wave20-10`.
