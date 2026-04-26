# Wave 18 Task 06 — Autonomous PLAN Field Inference v0

## Goal

Begin conservative autonomous PLAN.lisp field inference.

The system should infer missing PLAN DAG fields from directive text + historical evidence only when confidence is high. Otherwise it must return suggestions and refuse to mutate.

This is not arbitrary PLAN semantic interpretation.

## Dependency

Run after Wave18-05 if plan files are being edited serially.

## Ownership

Expected files:

- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`
- `crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs`
- `crates/missiond-daemon/src/handlers/knowledge/evidence_collector.rs`
- `crates/missiond-mcp/src/tools/knowledge/plan.rs`

Do not modify Lisp.

## Requirements

1. Add optional input:

   - `infer_plan_fields`: `off | preview | apply_safe`
   - default `off`

2. Supported fields for v0:

   - `target`
   - `dispatch_strategy`
   - `target_project`
   - `owned_files`
   - `acceptance_mode`
   - `workstation_dispatch`

3. Evidence sources:

   - directive references / compiled_from
   - plan sexp existing hints
   - evidence sidecar historical entries
   - workflow match rules if already available

4. Confidence:

   - each inferred field must carry `confidence`
   - `apply_safe` applies only fields above threshold and only if no caller-specified value exists
   - conflicts become suggestions, not mutations

5. Response:

   - `inference_status`
   - `inferred_fields`
   - `suggested_fields`
   - `conflicts`

6. No LLM call in this task. Deterministic heuristics only.

## Tests

Add tests for:

- preview infers target from clear existing plan hint
- preview infers owned_files from evidence
- apply_safe does not override explicit caller value
- low confidence becomes suggestion
- conflict reported
- default off preserves current behavior

## Acceptance Commands

```bash
cargo test -p missiond-daemon handlers::knowledge::plan::tests
cargo test -p missiond-daemon handlers::knowledge::plan_dag::tests
cargo test -p missiond-daemon handlers::knowledge::evidence_collector::tests
cargo test -p missiond-daemon
cargo test -p missiond-mcp --lib
cargo build --workspace
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check
```

## Commit

```bash
git add crates/missiond-daemon/src/handlers/knowledge/plan.rs \
        crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs \
        crates/missiond-daemon/src/handlers/knowledge/evidence_collector.rs \
        crates/missiond-mcp/src/tools/knowledge/plan.rs
git commit -m "feat(plan): infer safe PLAN fields deterministically"
```

Only stage files actually modified.

## Report

Return:

- Commit hash.
- Inference fields and confidence rule.
- Non-goals preserved.
- Tests and acceptance results.
