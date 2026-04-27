# Wave 24 Parallel Dispatch Index

Wave 24 turns session traces into a dry-run router-policy loop. The runtime
default remains ClaudeCode / existing MissionD dispatch. Router output is
advisory unless a task explicitly says otherwise.

Shared ledgers:

- Shared memory: `.missiond/tasks/wave24/shared-memory.lisp`
- Session trace: `.missiond/tasks/wave24/session-trace.lisp`

## Global Rules

- Use the `.lisp` task contract as SSOT; rendered Markdown is only the execution
  view.
- Append claims/completions to shared memory.
- Append session-trace facts only when the rendered task says
  `session_trace_writable: true`.
- Do not stage Wave 24 task files from inside code tasks.
- Do not turn router dry-run into runtime replacement in this wave.

## Group 0 — Archive

Run first:

- `wave24-00-archive-wave23-artifacts`

This should commit the remaining Wave23 task contracts, rendered briefs,
reports, and shared memory. It must not stage Wave24 files.

## Group 1 — Foundations

Run after Group 0. These can run in parallel:

- `wave24-01-router-policy-schema-v1`
- `wave24-02-trace-corpus-index-v0`

Expected outputs:

- router policy schema + checker + seed policy
- read-only trace corpus indexer

Conflict note: both are scripts/schema tasks but have disjoint primary write
sets. If either needs shared parser changes, coordinate through shared memory.

## Group 2 — Recommendation CLI

Run after Group 1:

- `wave24-03-router-recommendation-cli-v0`

This consumes task contract + policy + trace index and emits an explainable
recommendation. It must remain read-only and dry-run only.

## Group 3 — Surfaces

Run after Group 2. These may run in parallel if the agents respect write scope:

- `wave24-04-plan-router-dry-run-surface-v0`
- `wave24-05-renderer-router-context-v0`

Expected outputs:

- `mission_plan(action=execute)` dry-run response block with `applied=false`
- rendered brief router-context section that is explicitly advisory

No runtime backend replacement is allowed.

## Group 4 — Cross-Wave Smoke

Run after Group 3:

- `wave24-06-router-dry-run-smoke-v0`

This pins the full advisory chain:

trace index -> recommendation -> renderer -> plan dry-run response

Invariant list:

- no LLM call
- no spawn
- no mutating git
- `dry_run_only=true`
- `applied=false`
- existing dispatch behavior unchanged

## Group 5 — Codex Blueprint Lane

Codex owns this after all code tasks commit:

- `wave24-07-lisp-backfill-router-dry-run-status`

Do not dispatch this to ClaudeCode unless explicitly redirected. It must mark
router dry-run code as code-aligned only where it actually landed, and keep
runtime router replacement / frontend Lisp as pending.

## Suggested Operator Order

1. Send `wave24-00`.
2. Send `wave24-01` and `wave24-02` in parallel.
3. Send `wave24-03`.
4. Send `wave24-04` and `wave24-05` in parallel.
5. Send `wave24-06`.
6. Return to Codex for `wave24-07`.
