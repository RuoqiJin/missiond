# wave19-02-task-contract-verifier-v1 — Task contract verifier v1

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave19/wave19-02-task-contract-verifier-v1.lisp`

## Machine Contract

- kind: `code-alignment`
- status: `ready`
- owner: `claudecode`
- dispatch_strategy: `agent-team`
- depends_on: `wave19-00-machine-contract-pilot`

## Goal

Add a verifier that checks a task Lisp contract against git state and a completed commit, so task.lisp becomes an enforceable dispatch contract rather than only a rendered brief.

## Ownership

- `scripts/verify-task-contract.mjs`
- `scripts/check-task-contract.mjs`
- `scripts/lib/missiond_lisp.mjs`
- `.missiond/tasks/schema/task-contract-v1.lisp`

## Must Not Touch

- `crates/**`
- `.missiond/v2/*.lisp`
- `.missiond/claudecode/wave18-*.md`

## Requirements

1. Use agent-team if useful: 使用 agent-team提高效率.
2. Add scripts/verify-task-contract.mjs; it must read one task.lisp and verify commit hash, commit message, changed files subset of :write-scope when :scope-check is write-scope-only, and no changed file overlaps :must-not-touch.
3. Support --commit <hash>, --json, and --dry-fixture.
4. Keep the existing checker backward compatible; only extend it if needed for shared helper reuse.
5. Do not require a ClaudeCode report file in v1; report-contract is a separate task.
6. Verifier must be read-only except normal process stdout/stderr; no git add, commit, reset, checkout, stash, push, merge, or rebase.

## Acceptance Commands

```bash
node scripts/check-task-contract.mjs --all
node scripts/verify-task-contract.mjs --dry-fixture
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check -- scripts/verify-task-contract.mjs scripts/check-task-contract.mjs scripts/lib/missiond_lisp.mjs .missiond/tasks/schema/task-contract-v1.lisp
```

## Commit

After acceptance, commit only files inside the declared write scope.

```bash
git add "scripts/verify-task-contract.mjs" \
        "scripts/check-task-contract.mjs" \
        "scripts/lib/missiond_lisp.mjs" \
        ".missiond/tasks/schema/task-contract-v1.lisp"
git commit -m "feat(tasks): verify task contracts against commits"
```

Scope check: `write-scope-only`.

## Report

- `Commit hash.`
- `Verifier CLI synopsis.`
- `Dry-fixture coverage list.`
- `Acceptance command results.`

