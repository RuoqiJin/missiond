# Wave 17 Task 01 — PLAN DAG Paused Resume Hook v0

## Goal

Close the Wave16 paused-node gap.

Wave16 can pause a PLAN DAG node on `:review-gate "question-event"` and emit a deterministic review question. This task adds the explicit resume hook and listener route so an approved `QuestionEvent::Resolved` can re-dispatch the paused node.

This is not general auto-approval. It only resumes nodes that were paused by a deterministic PLAN node review id.

## Ownership

Expected files:

- `crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs`
- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`
- `crates/missiond-daemon/src/handlers/knowledge/review_gate.rs`
- `crates/missiond-daemon/src/bus/v2_subscribers.rs`
- `crates/missiond-mcp/src/tools/knowledge/plan.rs`

Do not modify Lisp. Wave17 Lisp backfill is a later task.

## Requirements

1. Support an explicit resume input on `mission_plan(action=execute)`:

   - `resume_review_question_id`
   - `resume_review_decision`: `approved | rejected | needs_changes`
   - optional `resume_actor`
   - optional `resume_note`

2. Recognize Wave16 PLAN node review ids:

   ```text
   review:plan:<plan_id>:v<version>:plan-node:<node-hash>
   ```

   Follow the exact Wave16 id derivation in code. If the actual shape differs, use the code truth and report it.

3. Behavior:

   - `approved`: resume exactly the paused node and dispatch it.
   - `rejected`: keep node paused/failed according to existing failure-policy; do not dispatch.
   - `needs_changes`: keep node paused and surface next_step.

4. Validation:

   - plan id matches
   - plan version matches
   - node hash maps to exactly one paused node
   - node was paused because of review gate
   - plan status still allows execution/resume

5. Listener integration:

   - Extend the Wave16 review resolution subscriber so approved `QuestionEvent::Resolved` for plan-node ids calls the same resume helper.
   - Non-plan-node review ids keep existing behavior.
   - Unknown resolution strings are ignored/warned, never dispatched.

6. Evidence:

   - record review resume decision
   - record resumed node dispatch attempt
   - include event ref if available, otherwise unavailable reason

7. No broad PLAN reinterpretation. Only resume existing paused node state.

## Tests

Add tests for:

- parse plan-node review id
- approved resumes paused node and dispatches
- rejected does not dispatch
- needs_changes stays paused with next_step
- stale version rejected
- hash with no paused node rejected
- listener planner maps approved event to resume request

## Acceptance Commands

```bash
cargo test -p missiond-daemon handlers::knowledge::plan_dag::tests
cargo test -p missiond-daemon handlers::knowledge::plan::tests
cargo test -p missiond-daemon handlers::knowledge::review_gate::tests
cargo test -p missiond-daemon bus::v2_subscribers::tests
cargo test -p missiond-daemon
cargo test -p missiond-mcp --lib
cargo build --workspace
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check
```

If `bus::v2_subscribers::tests` does not exist, run the exact targeted tests you add and report the module path.

## Commit

```bash
git add crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs \
        crates/missiond-daemon/src/handlers/knowledge/plan.rs \
        crates/missiond-daemon/src/handlers/knowledge/review_gate.rs \
        crates/missiond-daemon/src/bus/v2_subscribers.rs \
        crates/missiond-mcp/src/tools/knowledge/plan.rs
git commit -m "feat(plan): resume paused DAG nodes after review"
```

Only stage files actually modified.

## Report

Return:

- Commit hash.
- Final resume input contract.
- Listener routing behavior.
- Evidence shape.
- Tests and acceptance results.
