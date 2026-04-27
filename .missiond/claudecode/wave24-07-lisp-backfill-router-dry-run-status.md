# wave24-07-lisp-backfill-router-dry-run-status — Lisp backfill router dry-run status

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave24/wave24-07-lisp-backfill-router-dry-run-status.lisp`

## Machine Contract

- kind: `lisp-only`
- status: `ready`
- owner: `codex-architect`
- dispatch_strategy: `manual`
- depends_on: `wave24-01-router-policy-schema-v1`, `wave24-02-trace-corpus-index-v0`, `wave24-03-router-recommendation-cli-v0`, `wave24-04-plan-router-dry-run-surface-v0`, `wave24-05-renderer-router-context-v0`, `wave24-06-router-dry-run-smoke-v0`
- shared_memory: `.missiond/tasks/wave24/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave24/reports/wave24-07-lisp-backfill-router-dry-run-status.report.lisp`
- session_trace: `.missiond/tasks/wave24/session-trace.lisp`
- session_trace_writable: `false`

## Goal

Codex-owned architecture/status backfill: record Wave 24 router dry-run artifacts in v2 Lisp without claiming runtime backend replacement.

## Ownership

- `.missiond/v2/intent-machine-contract.lisp`
- `.missiond/v2/intent-workstation-policy.lisp`
- `.missiond/v2/intent-pillar-source-index.lisp`
- `.missiond/v2/intent-flow.lisp`
- `.missiond/v2/intent-intent-layer.lisp`
- `.missiond/v2/intent-tools.lisp`
- `.missiond/v2/intent.lisp`

## Must Not Touch

- `crates/**`
- `scripts/**`
- `.missiond/tasks/**`
- `.missiond/claudecode/**`

## Requirements

1. Do not delegate this blueprint/status task to ClaudeCode; Codex owns it.
2. Backfill only committed Wave24 facts.
3. Mark router recommendation CLI and mission_plan dry-run surface separately from future runtime router apply.
4. Keep frontend Lisp explicitly postponed unless a future wave starts it.

## Acceptance Commands

```bash
node scripts/check-architecture-lisp.mjs --all-v2
node scripts/check-task-contract.mjs --all
git diff --check -- .missiond/v2/intent-machine-contract.lisp .missiond/v2/intent-workstation-policy.lisp .missiond/v2/intent-pillar-source-index.lisp .missiond/v2/intent-flow.lisp .missiond/v2/intent-intent-layer.lisp .missiond/v2/intent-tools.lisp .missiond/v2/intent.lisp
```

## Shared Memory

Coordination ledger: `.missiond/tasks/wave24/shared-memory.lisp` (schema `missiond.shared-memory.v1`).

- Append a `claim` entry before starting work; append `observation` / `blocker` while running; append `completion` when done.
- Entries are append-only S-expressions; never edit prior entries — record fixes via a new `correction` entry.
- `:touched` paths in your entries must stay inside this task `:write-scope`.

Validate with:

```bash
node scripts/check-task-memory.mjs .missiond/tasks/wave24/shared-memory.lisp
```

## Report Contract

Expected machine-readable report: `.missiond/tasks/wave24/reports/wave24-07-lisp-backfill-router-dry-run-status.report.lisp` (schema `missiond.report-contract.v1`).

- Required fields: `:schema`, `:task_id`, `:status`, `:commit_hash`, `:files_changed`, `:acceptance_results`.
- `:status` must be one of `draft | in-progress | done | blocked | rejected`; `done` requires non-empty `:acceptance_results`.
- Free-form prose belongs in `:notes`; structural fields drive automated verification.
- Optional worker-explanation fields (prose only — facts live in `session-trace.lisp`):
  - `:time_sinks` — vector of strings or `(:label <s> [:duration_ms <int>] [:notes <s>])` entries.
  - `:major_decisions` — vector of strings or `(:decision <s> [:rationale <s>] [:trace_ref <s>])` entries.
  - `:unexpected_work` — vector of strings or `(:summary <s> [:trace_ref <s>])` entries.
  - `:blockers` — vector of strings or `(:summary <s> [:resolved <bool>] [:trace_ref <s>])` entries.
  - `:trace_refs` — vector of session-trace event ids or repo-relative paths linking back to factual telemetry.

Validate with:

```bash
node scripts/check-task-report.mjs .missiond/tasks/wave24/reports/wave24-07-lisp-backfill-router-dry-run-status.report.lisp
```

## Session Trace

Factual telemetry ledger: `.missiond/tasks/wave24/session-trace.lisp` (schema `missiond.session-trace.v1`).

- This file is the single source of truth for what happened: dispatch / start / read / edit / command / test / commit / complete / failure / retry / observation events.
- Worker prose explanations belong in the report contract's `:time_sinks` / `:major_decisions` / `:unexpected_work` / `:blockers` / `:trace_refs` fields, not here.
- This task is **not** `:session-trace-writable` (default). You MUST NOT write to `session-trace.lisp` — read it for context only. Telemetry for this task is recorded by MissionD or by tasks explicitly opted in via `:session-trace-writable true`.

Validate the ledger after any change with:

```bash
node scripts/check-session-trace.mjs .missiond/tasks/wave24/session-trace.lisp
```

## Commit

After acceptance, commit only files inside the declared write scope.

Preflight: confirm the repo-local `core.hooksPath` doctor is green so the shared `.githooks/pre-commit` hook also enforces the staged guard. Drift here is a preflight problem, not a hard error — the doctor is read-only; only `--install` mutates git config.

```bash
node scripts/check-missiond-hooks.mjs --json   # read-only doctor; reports preflight-drift on unset/wrong path
node scripts/install-missiond-hooks.mjs --install   # only run when the doctor reports drift; writes --local config only
```

Stage just the declared scope, run the pre-commit scoped-index guard, then commit:

```bash
git add ".missiond/v2/intent-machine-contract.lisp" \
        ".missiond/v2/intent-workstation-policy.lisp" \
        ".missiond/v2/intent-pillar-source-index.lisp" \
        ".missiond/v2/intent-flow.lisp" \
        ".missiond/v2/intent-intent-layer.lisp" \
        ".missiond/v2/intent-tools.lisp" \
        ".missiond/v2/intent.lisp"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave24/wave24-07-lisp-backfill-router-dry-run-status.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave24/wave24-07-lisp-backfill-router-dry-run-status.lisp \
  git commit -m "docs(v2): backfill wave24 router dry-run status"
```

Scope check: `write-scope-only`.

The `task-scope-guard --mode staged` step blocks the commit before the index is locked in if any staged path falls outside `:write-scope` or matches `:must-not-touch`. The `MISSIOND_TASK_CONTRACT` env var activates the same check from the shared `.githooks/pre-commit` hook (enable per clone with `node scripts/install-missiond-hooks.mjs --install`, equivalent to `git config core.hooksPath .githooks`).

Verify the commit against this contract (read-only, post-commit):

```bash
node scripts/verify-task-contract.mjs .missiond/tasks/wave24/wave24-07-lisp-backfill-router-dry-run-status.lisp
```

## Report

- `Commit hash.`
- `Anchors updated.`
- `Router dry-run vs runtime replacement distinction.`
- `Acceptance command results.`

