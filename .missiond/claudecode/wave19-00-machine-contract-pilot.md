# wave19-00-machine-contract-pilot — Machine Contract Pilot — render Lisp task to ClaudeCode brief

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave19/wave19-00-machine-contract-pilot.lisp`

## Machine Contract

- kind: `docs`
- status: `ready`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`

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

## Commit

After acceptance, commit only files inside the declared write scope.

```bash
git add "docs/machine-contract-pilot.md"
git commit -m "docs(task): add machine contract pilot note"
```

Scope check: `write-scope-only`.

## Report

- `Commit hash.`
- `Rendered task brief path.`
- `Files changed.`
- `Acceptance command results.`

