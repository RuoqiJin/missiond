# Wave 15 Task 03 — Make Source Checker Shard-Aware

## Goal

After Wave15-02 creates L2 shard files, make the architecture checker aware of shard references and source-index resolution.

The checker should fail fast when the source-index points to a missing shard file or duplicate section identity.

## Dependency

Start only after Wave15-02 is committed.

## Ownership

Primary:

- `scripts/check-architecture-lisp.mjs`
- `.missiond/v2/architecture-dsl.lisp`
- `.missiond/v2/intent-pillar-source-index.lisp`

Optional fixture/test files only if the checker already has local fixture conventions.

Do not modify Rust, SQL, Cargo, or non-index Lisp content.

## Requirements

1. `node scripts/check-architecture-lisp.mjs --all-v2` must include or resolve the new L2 shard files.

2. Add checks for:

   - Every source-index file reference exists.
   - Every referenced shard is under `.missiond/v2/`.
   - Every stable section id is unique across parent files and shard files.
   - `:compression-safe?` remains one of the Wave14 enum values.

3. Keep backwards compatibility with the existing 14-file checker behavior.

   The checker may discover shard files through `intent-pillar-source-index.lisp` instead of hardcoding every shard path, but the final report should make it clear how many files were checked.

4. If you update `architecture-dsl.lisp`, add only rule/checker documentation. Do not alter semantic architecture content.

## Acceptance Commands

Run:

```bash
node scripts/check-architecture-lisp.mjs --all-v2
node scripts/check-architecture-lisp.mjs --help >/tmp/missiond-checker-help.txt || true
git diff --check -- scripts/check-architecture-lisp.mjs .missiond/v2/architecture-dsl.lisp .missiond/v2/intent-pillar-source-index.lisp
```

If the checker has fixture mode, also run all checker fixtures.

## Commit

After acceptance:

```bash
git add scripts/check-architecture-lisp.mjs .missiond/v2/architecture-dsl.lisp .missiond/v2/intent-pillar-source-index.lisp
git commit -m "chore(v2): make source checker shard-aware"
```

## Report

Return:

- Commit hash.
- File discovery strategy.
- New validations added.
- Checker output, including checked file count.
