# wave25-01-router-policy-corpus-evaluator-v0 — Router policy corpus evaluator v0

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave25/wave25-01-router-policy-corpus-evaluator-v0.lisp`

## Machine Contract

- kind: `code-alignment`
- status: `ready`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- depends_on: `wave25-00-archive-wave24-artifacts`
- shared_memory: `.missiond/tasks/wave25/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave25/reports/wave25-01-router-policy-corpus-evaluator-v0.report.lisp`
- session_trace: `.missiond/tasks/wave25/session-trace.lisp`
- session_trace_writable: `true`

## Goal

Add a read-only evaluator CLI that runs the dry-run router recommendation across a corpus of task contracts and trace-index evidence, producing a measurable policy report without changing runtime dispatch.

## Ownership

- `scripts/evaluate-router-policy-corpus.mjs`

## Must Not Touch

- `crates/**`
- `scripts/recommend-task-backend.mjs`
- `scripts/build-session-trace-index.mjs`
- `scripts/check-router-policy.mjs`
- `.missiond/v2/**`
- `.missiond/tasks/schema/*.lisp`
- `.missiond/tasks/wave24/**`
- `.missiond/tasks/wave25/wave25-*.lisp`
- `.missiond/claudecode/**`

## Requirements

1. CLI shape: node scripts/evaluate-router-policy-corpus.mjs --policy <router-policy.lisp> [--tasks-root .missiond/tasks] [--trace-index <json>] [--json] [--dry-fixture].
2. The evaluator must read task contracts, reuse the existing router recommendation logic if exported cleanly or implement a small deterministic wrapper without shelling out.
3. Output schema must be missiond.router-policy-evaluation.v0 with totals, by_backend, by_confidence, fallback_count, rejected_count, per_task rows, and policy_path.
4. If --trace-index is absent, build the index in-process from existing session traces, not by spawning build-session-trace-index.mjs.
5. Hard guarantees: read-only, no shell, no git, no LLM, no HTTP, no runtime dispatch mutation.
6. Include dry fixtures covering empty corpus, multi-task corpus, fallback rows, rejected policy, and deterministic stable JSON.

## Acceptance Commands

```bash
node scripts/evaluate-router-policy-corpus.mjs --dry-fixture
node scripts/evaluate-router-policy-corpus.mjs --policy .missiond/router/router-policy-v1.lisp --tasks-root .missiond/tasks --json
node scripts/check-router-policy.mjs .missiond/router/router-policy-v1.lisp
node scripts/check-task-contract.mjs --all
git diff --check -- scripts/evaluate-router-policy-corpus.mjs
```

## Shared Memory

Coordination ledger: `.missiond/tasks/wave25/shared-memory.lisp` (schema `missiond.shared-memory.v1`).

- Append a `claim` entry before starting work; append `observation` / `blocker` while running; append `completion` when done.
- Entries are append-only S-expressions; never edit prior entries — record fixes via a new `correction` entry.
- `:touched` paths in your entries must stay inside this task `:write-scope`.

Validate with:

```bash
node scripts/check-task-memory.mjs .missiond/tasks/wave25/shared-memory.lisp
```

## Report Contract

Expected machine-readable report: `.missiond/tasks/wave25/reports/wave25-01-router-policy-corpus-evaluator-v0.report.lisp` (schema `missiond.report-contract.v1`).

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
node scripts/check-task-report.mjs .missiond/tasks/wave25/reports/wave25-01-router-policy-corpus-evaluator-v0.report.lisp
```

## Session Trace

Factual telemetry ledger: `.missiond/tasks/wave25/session-trace.lisp` (schema `missiond.session-trace.v1`).

- This file is the single source of truth for what happened: dispatch / start / read / edit / command / test / commit / complete / failure / retry / observation events.
- Worker prose explanations belong in the report contract's `:time_sinks` / `:major_decisions` / `:unexpected_work` / `:blockers` / `:trace_refs` fields, not here.
- This task is `:session-trace-writable true`: you MAY append `(trace-event ...)` entries to the ledger as factual coordination output, in addition to your declared `:write-scope`. Entries must follow the schema (required `:id` `:seq` `:at` `:task` `:backend` `:kind` `:summary`).
- Treat the trace ledger as an append-only journal: never edit prior events; record corrections as new events that reference the prior `:id` via `:trace_refs`.

Validate the ledger after any change with:

```bash
node scripts/check-session-trace.mjs .missiond/tasks/wave25/session-trace.lisp
```

## Router Policy (advisory)

Dry-run router-policy ledger: `.missiond/router/router-policy-v1.lisp` (schema `missiond.router-policy.v1`).

- This section is **advisory** and **dry-run only**. The policy file is informational; it captures backend recommendations distilled from prior session-trace observations, but **runtime dispatch is unchanged** — ClaudeCode remains the live backend for this task.
- The brief surfaces the policy path so human readers and ClaudeCode workers can consult the recommendations; it does not instruct the worker to switch backend, alter the dispatch strategy, or run the recommendation CLI.
- Source: explicit `:router-policy-path` on the task contract.

Inspect the policy with the read-only checker (the renderer itself does not execute the policy or shell out to the recommendation CLI):

```bash
node scripts/check-router-policy.mjs .missiond/router/router-policy-v1.lisp
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
git add "scripts/evaluate-router-policy-corpus.mjs"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave25/wave25-01-router-policy-corpus-evaluator-v0.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave25/wave25-01-router-policy-corpus-evaluator-v0.lisp \
  git commit -m "feat(tasks): evaluate router policy over trace corpus"
```

Scope check: `write-scope-only`.

The `task-scope-guard --mode staged` step blocks the commit before the index is locked in if any staged path falls outside `:write-scope` or matches `:must-not-touch`. The `MISSIOND_TASK_CONTRACT` env var activates the same check from the shared `.githooks/pre-commit` hook (enable per clone with `node scripts/install-missiond-hooks.mjs --install`, equivalent to `git config core.hooksPath .githooks`).

Verify the commit against this contract (read-only, post-commit):

```bash
node scripts/verify-task-contract.mjs .missiond/tasks/wave25/wave25-01-router-policy-corpus-evaluator-v0.lisp
```

## Report

- `Commit hash.`
- `Evaluation JSON shape.`
- `Read-only/no-shell/no-git proof.`
- `Acceptance command results.`

