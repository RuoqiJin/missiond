# Wave 17 Task 09 — Lisp Backfill for Wave17 Status

## Goal

Backfill `.missiond/v2/*.lisp` after Wave17 code tasks complete.

This is truth synchronization only. Do not invent architecture beyond committed code.

## Dependency

Run after the Wave17 implementation tasks that actually landed.

Use the resident Lisp session. Avoid `claude -p`.

## Ownership

You may modify only `.missiond/v2/*.lisp`.

Likely files/shards:

- `.missiond/v2/intent.lisp`
- `.missiond/v2/intent-flow.lisp`
- `.missiond/v2/intent-intent-layer.lisp`
- `.missiond/v2/intent-tools.lisp`
- `.missiond/v2/intent-memory.lisp`
- `.missiond/v2/intent-worker.lisp`
- `.missiond/v2/intent-plan-dag.lisp`
- `.missiond/v2/intent-workstation-policy.lisp`
- `.missiond/v2/intent-directive-artifacts.lisp`
- `.missiond/v2/intent-pillar-source-index.lisp`
- `.missiond/v2/architecture-dsl.lisp` only if checker/status taxonomy changed

Do not modify Rust, SQL, JS, Cargo, or task docs.

## Requirements

1. Reflect committed Wave17 facts only.

2. Update source-index entries if new anchors were added.

3. Preserve existing section ids and shard references.

4. Explicitly list remaining pending items.

5. Keep frontend Lisp postponed. Do not start `intent-ui.lisp` in this task.

## Acceptance Commands

```bash
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check -- .missiond/v2
```

## Commit

```bash
git add .missiond/v2
git commit -m "docs(v2): backfill wave17 implementation status"
```

## Report

Return:

- Commit hash.
- Files changed.
- Wave17 items reflected.
- Remaining pending items.
- Checker and diff-check results.
