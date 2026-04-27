# Wave 23 Parallel Dispatch Index

Wave 23 adds `session-trace` as MissionD-owned factual telemetry. Rendered
`.md` files are ClaudeCode execution views; `.missiond/tasks/wave23/*.lisp`
remains the SSOT.

Shared ledgers:

- Shared memory: `.missiond/tasks/wave23/shared-memory.lisp`
- Session trace: `.missiond/tasks/wave23/session-trace.lisp`

## Ownership Rule

ClaudeCode should handle code-alignment, scripts, tests, and mechanical
verification tasks.

Codex owns blueprint/design tasks:

- `wave23-07-router-policy-draft-from-trace-v0`
- `wave23-08-lisp-backfill-wave23-status`

Do not dispatch those two to ClaudeCode.

## Group 0 — Archive

Run first via ClaudeCode:

- `wave23-00-archive-wave22-task-artifacts`

Commits Wave22 task contracts, rendered briefs, reports, and shared memory.

## Group 1 — Trace Foundation

Run after Group 0:

- `wave23-01-session-trace-schema-v0`

This creates the schema/checker/seed trace ledger. It should land before any
other trace-aware task.

## Group 2 — Renderer / Verifier Scripts

Can run after Group 1, with script conflicts coordinated:

- `wave23-02-renderer-report-trace-fields-v1`
- `wave23-03-task-run-verifier-trace-v1`
- `wave23-06-trace-summary-analyzer-v0`

Potential conflict: `check-task-report.mjs`, `check-session-trace.mjs`,
`scripts/lib/missiond_lisp.mjs`. If agents need the same helper, serialize or
have the later agent rebase on the earlier commit.

## Group 3 — Runtime Trace Integration

Run after Group 1:

- `wave23-04-execution-session-trace-integration-v0`
- `wave23-05-plan-workstation-session-trace-v0`

Order:

1. `wave23-04` first, because it adds execution-side trace append support.
2. `wave23-05` second, because it forwards trace paths through plan/workstation.

## Group 4 — Codex Blueprint Lane

Codex does these after trace schema/analyzer facts are available:

- `wave23-07-router-policy-draft-from-trace-v0`
- `wave23-08-lisp-backfill-wave23-status`

The router policy must stay architecture-designed only; Wave23 does not replace
ClaudeCode.

## Suggested Operator Order

1. Send `wave23-00`.
2. Send `wave23-01`.
3. Send `wave23-02`, `wave23-03`, `wave23-06` with script-conflict awareness.
4. Send `wave23-04`, then `wave23-05`.
5. Hand back to Codex for `wave23-07` and `wave23-08`.
