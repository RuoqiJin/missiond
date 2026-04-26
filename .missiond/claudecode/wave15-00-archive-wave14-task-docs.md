# Wave 15 Task 00 — Archive Wave14 Task Briefs

## Goal

Archive the Wave14 task brief files that were left untracked after Wave14 completed.

This is a bookkeeping task only. Do not modify Rust, Lisp, SQL, JavaScript, Cargo metadata, or Wave15 task files.

## Ownership

You own only:

- `.missiond/claudecode/wave14-*.md`

You must not stage or commit:

- `.missiond/claudecode/wave15-*.md`
- Any source file
- Any `.missiond/v2/*.lisp`

## Steps

1. Inspect the current untracked Wave14 task docs:

   ```bash
   git status --short -- .missiond/claudecode/wave14-*.md
   ```

2. Stage only Wave14 task docs:

   ```bash
   git add .missiond/claudecode/wave14-*.md
   ```

3. Confirm the staged set contains only Wave14 task docs:

   ```bash
   git diff --cached --name-only
   ```

4. Commit:

   ```bash
   git commit -m "chore(wave14): archive task briefs"
   ```

## Acceptance

Run:

```bash
git status --short
git log -1 --oneline
```

Expected:

- The new commit contains only `.missiond/claudecode/wave14-*.md`.
- Wave15 task docs remain untracked.
- No source file is staged or committed.

## Report

Return:

- Commit hash.
- Exact staged file list.
- Confirmation that Wave15 docs were not staged.
