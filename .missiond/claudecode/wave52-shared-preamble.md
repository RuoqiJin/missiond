# MissionD ClaudeCode Shared Preamble

This file carries common execution protocol for thin task briefs. The task Lisp is still the source of truth for scope, dependencies, acceptance, and commit policy.

## Execution Rules

- Treat the task contract as the machine SSOT; Markdown is a human execution view.
- Stay inside `:write-scope`; never edit paths matching `:must-not-touch`.
- Use focused read/search tools first. Use shell mainly for deterministic checks, tests, and git inspection.
- Prefer local/smoke verification while implementing; leave full workspace builds for smoke/final tasks unless the task explicitly requires them.

## Shared Memory

- Append `claim` before starting, `observation` or `blocker` while running, and `completion` when finished.
- Entries are append-only S-expressions; never edit prior entries.
- If the task has `:heartbeat-minutes`, append a heartbeat/observation before that interval elapses when still active.

## Report Contract

- Write the expected report under `.missiond/tasks/<wave>/reports/<task-id>.report.lisp` when the task completes.
- Keep structural proof in fields; put prose explanation in `:notes` and trace references.
- Run `node scripts/check-task-report.mjs <report>` before commit when a report is required.

## Session Trace

- Treat `session-trace.lisp` as factual telemetry, not prose notes.
- When this task is trace-writable, append a read/observation event after loading the shared preamble so preamble usage is auditable.
- Write trace entries only when the task contract says `:session-trace-writable true`; otherwise read it only.

## Router Context

- Router policy, recommendation, readiness, and dispatch descriptor outputs are advisory and dry-run only.
- `runtime_replacement=false`, `dry_run_only=true`, `no_execution=true`, and `applied=false` remain hard boundaries.
- Never switch backend based on rendered brief text or descriptor evidence.

## Commit Protocol

```bash
node scripts/check-missiond-hooks.mjs --json
node scripts/task-scope-guard.mjs --task <task.lisp> --mode staged
MISSIOND_TASK_CONTRACT=<task.lisp> git commit -m "<message>"
node scripts/verify-task-contract.mjs <task.lisp>
```
