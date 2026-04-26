# wave19-05-renderer-dispatch-brief-v1 — Renderer dispatch brief v1

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave19/wave19-05-renderer-dispatch-brief-v1.lisp`

## Machine Contract

- kind: `code-alignment`
- status: `ready`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- depends_on: `wave19-02-task-contract-verifier-v1`, `wave19-03-report-contract-v1`, `wave19-04-shared-memory-ledger-v0`

## Goal

Upgrade the Lisp-to-Markdown renderer so ClaudeCode briefs carry enough machine-contract context for scoped commits and shared-memory handoff.

## Ownership

- `scripts/render-claudecode-task.mjs`
- `.missiond/tasks/schema/task-contract-v1.lisp`
- `.missiond/tasks/wave19/wave19-00-machine-contract-pilot.lisp`
- `.missiond/claudecode/wave19-00-machine-contract-pilot.md`

## Must Not Touch

- `crates/**`
- `.missiond/v2/*.lisp`
- `scripts/check-task-contract.mjs`
- `scripts/verify-task-contract.mjs`
- `scripts/check-task-report.mjs`
- `scripts/check-task-memory.mjs`

## Requirements

1. Render depends_on, dispatch_strategy, shared-memory path, report-contract expectation, and verify-task-contract command when available.
2. If :dispatch-strategy is agent-team, render the exact literal 使用 agent-team提高效率 once.
3. Keep existing output backward compatible for current fields.
4. Re-render wave19-00 pilot with --force as a golden example.

## Acceptance Commands

```bash
node scripts/check-task-contract.mjs --all
node scripts/render-claudecode-task.mjs --force .missiond/tasks/wave19/wave19-00-machine-contract-pilot.lisp
git diff --check -- scripts/render-claudecode-task.mjs .missiond/tasks/schema/task-contract-v1.lisp .missiond/tasks/wave19/wave19-00-machine-contract-pilot.lisp .missiond/claudecode/wave19-00-machine-contract-pilot.md
```

## Commit

After acceptance, commit only files inside the declared write scope.

```bash
git add "scripts/render-claudecode-task.mjs" \
        ".missiond/tasks/schema/task-contract-v1.lisp" \
        ".missiond/tasks/wave19/wave19-00-machine-contract-pilot.lisp" \
        ".missiond/claudecode/wave19-00-machine-contract-pilot.md"
git commit -m "feat(tasks): enrich rendered dispatch briefs"
```

Scope check: `write-scope-only`.

## Report

- `Commit hash.`
- `Rendered sections added.`
- `Backward compatibility notes.`
- `Acceptance command results.`

