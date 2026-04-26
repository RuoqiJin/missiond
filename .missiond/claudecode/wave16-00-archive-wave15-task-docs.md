# Wave 16 Task 00 — Archive Wave15 Task Briefs

## Goal

Archive the Wave15 task brief files left untracked after Wave15 completion.

This is bookkeeping only.

## Ownership

You own only:

- `.missiond/claudecode/wave15-*.md`

Do not stage or commit:

- `.missiond/claudecode/wave16-*.md`
- Rust / SQL / JS / Cargo files
- `.missiond/v2/*.lisp`

## Steps

```bash
git status --short -- .missiond/claudecode/wave15-*.md
git add .missiond/claudecode/wave15-*.md
git diff --cached --name-only
git commit -m "chore(wave15): archive task briefs"
```

Before committing, confirm the staged list contains only Wave15 task docs.

## Acceptance

```bash
git log -1 --oneline
git status --short
```

Expected:

- Commit contains only `.missiond/claudecode/wave15-*.md`.
- Wave16 task docs remain untracked.
- Source tree remains unchanged.

## Report

Return:

- Commit hash.
- Staged file list.
- Confirmation that Wave16 docs were not staged.
