# Wave 15 Task 06 — Lisp Backfill for Wave15 Status

## Goal

Backfill `.missiond/v2/*.lisp` after Wave15 code tasks complete.

This is status and source-index synchronization only. Do not invent new architecture beyond what Wave15 actually implemented.

## Dependency

Run after the relevant Wave15 code tasks have committed:

- Wave15-01 domain count fix
- Wave15-02 L2 shard split
- Wave15-03 shard-aware checker
- Wave15-04 review gate resolution v0, if completed
- Wave15-05 workstation dispatch v0, if completed

Use the resident Lisp session. Avoid `claude -p`.

## Ownership

You may modify only `.missiond/v2/*.lisp`.

Likely files:

- `.missiond/v2/intent.lisp`
- `.missiond/v2/intent-flow.lisp`
- `.missiond/v2/intent-intent-layer.lisp`
- `.missiond/v2/intent-tools.lisp`
- `.missiond/v2/intent-memory.lisp`
- `.missiond/v2/intent-worker.lisp`
- `.missiond/v2/intent-pillar-source-index.lisp`
- any new L2 shard files created by Wave15-02
- `.missiond/v2/architecture-dsl.lisp` only for checker rule status if Wave15-03 changed semantics

Do not modify Rust, SQL, JS, Cargo, or task docs.

## Requirements

1. Reflect only committed implementation truth.

2. Use the Wave14/Wave15 status taxonomy:

   - `code-aligned`
   - `code-aligned-partial`
   - `architecture-designed`
   - `implementation-target`
   - `operational-practice`
   - `protected`
   - `deprecated`

3. Update source-index entries for any moved or newly anchored sections.

4. If L2 shards exist, parent files should keep concise anchor stubs and cross-references.

5. Keep compression conservative:

   - Do not remove anchors.
   - Do not merge unrelated sections.
   - Do not rewrite event-bus protected content.

## Acceptance Commands

Run:

```bash
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check -- .missiond/v2
```

## Commit

After acceptance:

```bash
git add .missiond/v2
git commit -m "docs(v2): backfill wave15 implementation status"
```

## Report

Return:

- Commit hash.
- Files changed.
- Which Wave15 tasks were reflected.
- Which items remain pending.
- Checker and diff-check results.
