# wave19-00-machine-contract-pilot — Machine Contract Pilot — render Lisp task to ClaudeCode brief

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave19/wave19-00-machine-contract-pilot.lisp`

## Machine Contract

- kind: `docs`
- status: `ready`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- shared_memory: `.missiond/tasks/wave19/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave19/reports/wave19-00-machine-contract-pilot.report.lisp`

## Goal

Prove that a Lisp task-contract can serve as the machine-readable source of truth while Markdown remains a rendered ClaudeCode execution view.

## Ownership

- `docs/machine-contract-pilot.md`

## Must Not Touch

- `.missiond/v2/*.lisp`
- `crates/**`
- `scripts/**`

## Requirements

1. Create a short Markdown note explaining that this pilot was generated from a Lisp task contract.
2. Do not modify any source code.
3. Do not modify architecture Lisp files.
4. Keep the note under 80 lines.

## Acceptance Commands

```bash
git diff --check -- docs/machine-contract-pilot.md
node scripts/check-task-contract.mjs .missiond/tasks/wave19/wave19-00-machine-contract-pilot.lisp
```

## Shared Memory

Coordination ledger: `.missiond/tasks/wave19/shared-memory.lisp` (schema `missiond.shared-memory.v1`).

- Append a `claim` entry before starting work; append `observation` / `blocker` while running; append `completion` when done.
- Entries are append-only S-expressions; never edit prior entries — record fixes via a new `correction` entry.
- `:touched` paths in your entries must stay inside this task `:write-scope`.

Validate with:

```bash
node scripts/check-task-memory.mjs .missiond/tasks/wave19/shared-memory.lisp
```

## Report Contract

Expected machine-readable report: `.missiond/tasks/wave19/reports/wave19-00-machine-contract-pilot.report.lisp` (schema `missiond.report-contract.v1`).

- Required fields: `:schema`, `:task_id`, `:status`, `:commit_hash`, `:files_changed`, `:acceptance_results`.
- `:status` must be one of `draft | in-progress | done | blocked | rejected`; `done` requires non-empty `:acceptance_results`.
- Free-form prose belongs in `:notes`; structural fields drive automated verification.

Validate with:

```bash
node scripts/check-task-report.mjs .missiond/tasks/wave19/reports/wave19-00-machine-contract-pilot.report.lisp
```

## Commit

After acceptance, commit only files inside the declared write scope.

```bash
git add "docs/machine-contract-pilot.md"
git commit -m "docs(task): add machine contract pilot note"
```

Scope check: `write-scope-only`.

Verify the commit against this contract (read-only, post-commit):

```bash
node scripts/verify-task-contract.mjs .missiond/tasks/wave19/wave19-00-machine-contract-pilot.lisp
```

## Report

- `Commit hash.`
- `Rendered task brief path.`
- `Files changed.`
- `Acceptance command results.`

