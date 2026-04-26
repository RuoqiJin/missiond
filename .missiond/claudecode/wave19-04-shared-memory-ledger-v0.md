# wave19-04-shared-memory-ledger-v0 — Shared memory ledger v0

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave19/wave19-04-shared-memory-ledger-v0.lisp`

## Machine Contract

- kind: `code-alignment`
- status: `ready`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- depends_on: `wave19-00-machine-contract-pilot`

## Goal

Create a Lisp shared-memory ledger shape for parallel ClaudeCode agents: claims, observations, blockers, commits, and handoff pointers.

## Ownership

- `.missiond/tasks/schema/shared-memory-v1.lisp`
- `.missiond/tasks/wave19/shared-memory.lisp`
- `scripts/check-task-memory.mjs`

## Must Not Touch

- `crates/**`
- `.missiond/v2/*.lisp`
- `scripts/check-task-contract.mjs`
- `scripts/render-claudecode-task.mjs`

## Requirements

1. Define a data-Lisp ledger, not a prose note: entries must be S-expressions with stable heads such as claim, observation, blocker, completion, correction.
2. Add a checker with --dry-fixture; validate task ids, entry ids, timestamps or monotonic sequence numbers, touched files as repo-relative paths, and no duplicate entry ids.
3. Seed .missiond/tasks/wave19/shared-memory.lisp with a header and zero or one bootstrap observation; do not log fake task completions.
4. Document in the schema that agents append entries only inside their claimed write-scope, while the ledger itself is the one shared write target for coordination.

## Acceptance Commands

```bash
node scripts/check-task-memory.mjs --dry-fixture
node scripts/check-task-memory.mjs .missiond/tasks/wave19/shared-memory.lisp
node scripts/check-task-contract.mjs --all
git diff --check -- .missiond/tasks/schema/shared-memory-v1.lisp .missiond/tasks/wave19/shared-memory.lisp scripts/check-task-memory.mjs
```

## Commit

After acceptance, commit only files inside the declared write scope.

```bash
git add ".missiond/tasks/schema/shared-memory-v1.lisp" \
        ".missiond/tasks/wave19/shared-memory.lisp" \
        "scripts/check-task-memory.mjs"
git commit -m "feat(tasks): add shared memory ledger contract"
```

Scope check: `write-scope-only`.

## Report

- `Commit hash.`
- `Ledger entry types.`
- `Checker dry-fixture coverage.`
- `Acceptance command results.`

