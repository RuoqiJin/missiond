# Wave 17 Task 00 — Archive Wave16 Task Briefs

## Goal

Archive the Wave16 task brief files left untracked after Wave16 completion.

This is bookkeeping only.

## Ownership

You own only:

- `.missiond/claudecode/wave16-*.md`

Do not stage or commit:

- `.missiond/claudecode/wave17-*.md`
- Rust / SQL / JS / Cargo files
- `.missiond/v2/*.lisp`

## Steps

```bash
git status --short -- .missiond/claudecode/wave16-*.md
git add .missiond/claudecode/wave16-*.md
git diff --cached --name-only
git commit -m "chore(wave16): archive task briefs"
```

Before committing, confirm the staged list contains only Wave16 task docs.

## Acceptance

```bash
git log -1 --oneline
git status --short
```

Expected:

- Commit contains only `.missiond/claudecode/wave16-*.md`.
- Wave17 task docs remain untracked.
- Source tree remains unchanged.

## Report

Return:

- Commit hash.
- Staged file list.
- Confirmation that Wave17 docs were not staged.
