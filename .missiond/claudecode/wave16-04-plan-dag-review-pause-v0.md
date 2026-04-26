# Wave 16 Task 04 — PLAN DAG Review-Gate Pause v0

## Goal

Implement the first real `paused` node state in PLAN DAG runtime.

Current runtime has lifecycle/evidence support but review-gate paused remains architecture-designed. This task adds conservative behavior: a node with `:review-gate "question-event"` emits a review question and pauses instead of dispatching until a later resume.

This task does not need to implement the resume listener; Wave16-02 handles QuestionEvent resolution at the review-gate layer. This task only produces the paused state and deterministic question id.

## Dependency

Run after Wave16-03 if both are active, because both touch `plan_dag.rs`.

## Ownership

Expected files:

- `crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs`
- `crates/missiond-daemon/src/handlers/knowledge/review_gate.rs`
- `crates/missiond-mcp/src/tools/knowledge/plan.rs`

Do not modify workflow/directive handlers unless extracting shared review-gate helpers.

## Requirements

1. Parse node review gate hints:

   - `:review-gate "none"` (default)
   - `:review-gate "question-event"`
   - optional `:review-action`
   - optional `:review-text`

2. When a ready node has `question-event` gate:

   - emit `QuestionEvent::Created` with deterministic id
   - write typed evidence entry for paused review gate
   - mark node state `paused`
   - do not call the target tool

3. Response must surface:

   - paused node ids
   - review question ids
   - bus warning if publish failed

4. Bus failure semantics:

   - if question publish fails, node still pauses with warning; do not dispatch past a failed gate

5. Default behavior remains byte-compatible for nodes without review gate.

6. Do not implement automatic resume in this task.

## Tests

Add tests for:

- parser captures `:review-gate`
- node without gate dispatches as before
- question-event node pauses and does not dispatch
- bus failure produces warning but still pauses
- evidence entry has paused state and question id

## Acceptance Commands

```bash
cargo test -p missiond-daemon handlers::knowledge::plan_dag::tests
cargo test -p missiond-daemon handlers::knowledge::review_gate::tests
cargo test -p missiond-daemon handlers::knowledge::plan::tests
cargo test -p missiond-daemon
cargo test -p missiond-mcp --lib
cargo build --workspace
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check
```

## Commit

```bash
git add crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs \
        crates/missiond-daemon/src/handlers/knowledge/review_gate.rs \
        crates/missiond-mcp/src/tools/knowledge/plan.rs
git commit -m "feat(plan): pause DAG nodes for review gates"
```

## Report

Return:

- Commit hash.
- Review gate node contract.
- Paused state response fields.
- What remains for resume.
- Tests and acceptance results.
