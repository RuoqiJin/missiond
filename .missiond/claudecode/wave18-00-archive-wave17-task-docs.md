# Wave 18 Task 00 — Archive Wave17 Task Briefs

## Goal

Archive the Wave17 task brief files left untracked after Wave17 completion.

This is bookkeeping only.

## Ownership

You own only:

- `.missiond/claudecode/wave17-*.md`

Do not stage or commit:

- `.missiond/claudecode/wave18-*.md`
- Rust / SQL / JS / Cargo files
- `.missiond/v2/*.lisp`

## Steps

```bash
git status --short -- .missiond/claudecode/wave17-*.md
git add .missiond/claudecode/wave17-*.md
git diff --cached --name-only
git commit -m "chore(wave17): archive task briefs"
```

Before committing, confirm the staged list contains only Wave17 task docs.

## Acceptance

```bash
git log -1 --oneline
git status --short
```

Expected:

- Commit contains only `.missiond/claudecode/wave17-*.md`.
- Wave18 task docs remain untracked.
- Source tree remains unchanged.

## Report

Return:

- Commit hash.
- Staged file list.
- Confirmation that Wave18 docs were not staged.
