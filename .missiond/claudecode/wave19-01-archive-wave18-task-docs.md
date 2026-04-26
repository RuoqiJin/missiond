# wave19-01-archive-wave18-task-docs — Archive Wave 18 task briefs

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave19/wave19-01-archive-wave18-task-docs.lisp`

## Machine Contract

- kind: `docs`
- status: `ready`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`

## Goal

Commit the untracked Wave 18 task documents so the working tree starts Wave 19 from a clean baseline.

## Ownership

- `.missiond/claudecode/wave18-*.md`

## Must Not Touch

- `crates/**`
- `.missiond/v2/*.lisp`
- `.missiond/tasks/**`
- `scripts/**`

## Requirements

1. Stage only the existing .missiond/claudecode/wave18-*.md task documents.
2. Do not edit their contents unless git diff --check reports a whitespace problem.
3. Do not stage Wave 19 task contracts or rendered Wave 19 briefs.
4. Leave code and architecture Lisp untouched.

## Acceptance Commands

```bash
git diff --check -- .missiond/claudecode/wave18-*.md
git status --short -- .missiond/claudecode/wave18-*.md
```

## Commit

After acceptance, commit only files inside the declared write scope.

```bash
git add ".missiond/claudecode/wave18-*.md"
git commit -m "chore(wave18): archive task briefs"
```

Scope check: `write-scope-only`.

## Report

- `Commit hash.`
- `Number of Wave 18 files committed.`
- `Any files intentionally left untracked.`

