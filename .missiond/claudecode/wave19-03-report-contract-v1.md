# wave19-03-report-contract-v1 — ClaudeCode report contract v1

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave19/wave19-03-report-contract-v1.lisp`

## Machine Contract

- kind: `code-alignment`
- status: `ready`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- depends_on: `wave19-00-machine-contract-pilot`

## Goal

Define a machine-readable completion report Lisp contract and checker so ClaudeCode results can be verified without parsing prose.

## Ownership

- `.missiond/tasks/schema/report-contract-v1.lisp`
- `.missiond/tasks/wave19/reports/wave19-00-machine-contract-pilot.report.lisp`
- `scripts/check-task-report.mjs`

## Must Not Touch

- `crates/**`
- `.missiond/v2/*.lisp`
- `scripts/check-task-contract.mjs`
- `scripts/render-claudecode-task.mjs`

## Requirements

1. Add report-contract-v1.lisp describing fields: task_id, status, commit_hash, files_changed, acceptance_results, scope_deviations, notes.
2. Add scripts/check-task-report.mjs with --dry-fixture and single-file validation.
3. Create one sample report for wave19-00-machine-contract-pilot under .missiond/tasks/wave19/reports/ with status draft or done as appropriate; it is a schema example, not proof of task execution.
4. Checker must reject missing task_id, invalid status, empty acceptance_results when status=done, and absolute file paths.

## Acceptance Commands

```bash
node scripts/check-task-report.mjs --dry-fixture
node scripts/check-task-report.mjs .missiond/tasks/wave19/reports/wave19-00-machine-contract-pilot.report.lisp
node scripts/check-task-contract.mjs --all
git diff --check -- .missiond/tasks/schema/report-contract-v1.lisp .missiond/tasks/wave19/reports/wave19-00-machine-contract-pilot.report.lisp scripts/check-task-report.mjs
```

## Commit

After acceptance, commit only files inside the declared write scope.

```bash
git add ".missiond/tasks/schema/report-contract-v1.lisp" \
        ".missiond/tasks/wave19/reports/wave19-00-machine-contract-pilot.report.lisp" \
        "scripts/check-task-report.mjs"
git commit -m "feat(tasks): add machine-readable task reports"
```

Scope check: `write-scope-only`.

## Report

- `Commit hash.`
- `Report schema fields.`
- `Checker dry-fixture results.`
- `Acceptance command results.`

