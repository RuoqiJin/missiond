# Wave 15 Task 02 — Execute L2 Lisp Shard Split

## Goal

Execute the L2 shard split designed in Wave14 without changing semantics.

This is a Lisp-only structural refactor. The purpose is to reduce long-file pressure while preserving the canonical anchors and source-index discoverability.

## Critical Scheduling Constraint

Run this task only when no active code-alignment task is editing `.missiond/v2/*.lisp`.

Recommended order:

1. Wave15-00 archive docs.
2. Wave15-01 fix red integration test.
3. This task.
4. Wave15-03 shard-aware checker.

Use the resident Lisp ClaudeCode session if available. Do not use `claude -p`.

## Ownership

You may modify only `.missiond/v2/*.lisp`.

Expected new shard candidates from the Wave14 plan:

- `.missiond/v2/intent-execution-governance.lisp`
- `.missiond/v2/intent-directive-artifacts.lisp`
- `.missiond/v2/intent-plan-dag.lisp`
- `.missiond/v2/intent-capability-governance.lisp`
- `.missiond/v2/intent-workstation-policy.lisp`

Expected parent/index files:

- `.missiond/v2/intent-worker.lisp`
- `.missiond/v2/intent-memory.lisp`
- `.missiond/v2/intent-intent-layer.lisp`
- `.missiond/v2/intent-flow.lisp`
- `.missiond/v2/intent-tools.lisp`
- `.missiond/v2/intent-pillar-source-index.lisp`
- `.missiond/v2/intent.lisp`
- `.missiond/v2/architecture-dsl.lisp` only if required by checker/index semantics

Do not modify Rust, SQL, JS, Cargo files, or task docs besides this one.

## Non-Negotiable Rules

1. Preserve every stable `section-id` from the source-index.
2. Preserve all existing semantic assertions. This is move-and-reference, not redesign.
3. Parent files must keep short anchor stubs that point to the shard file and section ids.
4. The source-index must be updated so every moved section resolves to its new file.
5. Do not split the event-bus protected area unless the Wave14 source-index explicitly marked it compression-safe.
6. Avoid whole-file rewrites where possible. Use incremental edits.

## Suggested Shard Mapping

Use the Wave14 L2 plan as source of truth. If the plan differs from this sketch, follow the plan and report the difference.

- `intent-execution-governance.lisp`: mission_execution, scoped commit handoff, dispatch metadata, plan node event governance.
- `intent-directive-artifacts.lisp`: file-first alignment/directive/plan/workflow artifact contracts.
- `intent-plan-dag.lisp`: PLAN DAG node schema, scheduler state, failure policy, evidence sidecar relationship.
- `intent-capability-governance.lisp`: capability usage read model, semantic evidence lanes, merge review policy.
- `intent-workstation-policy.lisp`: resident Lisp workstation, fresh code-alignment sessions, agent-team hint, spawn-over-prompt policy.

## Acceptance Commands

Run:

```bash
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check -- .missiond/v2
```

Also run a source-index sanity check by search:

```bash
rg 'intent-execution-governance|intent-directive-artifacts|intent-plan-dag|intent-capability-governance|intent-workstation-policy' .missiond/v2
rg ':section-id|section-id' .missiond/v2/intent-pillar-source-index.lisp
```

## Commit

After acceptance:

```bash
git add .missiond/v2
git commit -m "docs(v2): split L2 architecture shards"
```

## Report

Return:

- Commit hash.
- New shard file list.
- Parent files changed.
- Source-index entries moved or added.
- Confirmation that no Rust/SQL/JS/Cargo files changed.
- Checker and diff-check results.
