# Wave 15 Task 04 — Review Gate Resolution / Resume v0

## Goal

Close the loop after Wave14 review-gate autocreate.

Wave14 can emit deterministic review questions for artifacts. This task adds an explicit, non-autonomous resume path: after a human/operator resolves a review, MissionD can advance the corresponding directive/plan/workflow state through existing manager surfaces.

This is not auto-approve. It is an explicit resolution bridge.

## Ownership

Expected files:

- `crates/missiond-daemon/src/handlers/knowledge/review_gate.rs`
- `crates/missiond-daemon/src/handlers/knowledge/directive.rs`
- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`
- `crates/missiond-daemon/src/handlers/knowledge/workflow.rs` only if workflow review gates are currently emitted there
- `crates/missiond-mcp/src/tools/knowledge/directive.rs`
- `crates/missiond-mcp/src/tools/knowledge/plan.rs`
- `crates/missiond-mcp/src/tools/knowledge/workflow.rs` only if schema needs workflow fields

Do not add a new MCP tool unless you can prove existing surfaces cannot express the operation.

## Requirements

1. Add an explicit review-resolution input shape to the existing relevant actions.

   Acceptable design:

   - `review_question_id`
   - `review_decision`: `approved | rejected | needs_changes`
   - `review_actor`
   - `review_note`

   Use existing action names if they already semantically match (`approve`, `mark`, `supersede`, etc.). Add a narrow action only if necessary.

2. Parse deterministic ids created by Wave14:

   ```text
   review:<scope>:<artifact_id>:v<version>:<action>[:<topic-hash>]
   ```

3. Validate scope and version before mutating state.

4. Behavior:

   - `approved`: resume/perform the intended manager transition.
   - `rejected`: keep artifact non-approved and record reason.
   - `needs_changes`: keep artifact in review/draft path and surface next_step.

5. Fail fast on mismatched ids, unsupported scope, unsupported action, or stale version.

6. Do not block waiting for a QuestionEvent answer. This task consumes explicit caller input only.

7. Bus failures must not corrupt DB state. If you publish follow-up events, failures become warnings.

## Tests

Add pure tests for deterministic id parsing and decision mapping.

Add handler tests for at least:

- approve via valid review id
- stale version rejection
- scope mismatch rejection
- rejected decision records reason without approving
- needs_changes surfaces next_step

## Acceptance Commands

Run:

```bash
cargo test -p missiond-daemon handlers::knowledge::review_gate::tests
cargo test -p missiond-daemon handlers::knowledge::directive::tests
cargo test -p missiond-daemon handlers::knowledge::plan::tests
cargo test -p missiond-daemon
cargo test -p missiond-mcp --lib
cargo build --workspace
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check
```

## Commit

After acceptance:

```bash
git add crates/missiond-daemon/src/handlers/knowledge/review_gate.rs \
        crates/missiond-daemon/src/handlers/knowledge/directive.rs \
        crates/missiond-daemon/src/handlers/knowledge/plan.rs \
        crates/missiond-daemon/src/handlers/knowledge/workflow.rs \
        crates/missiond-mcp/src/tools/knowledge/directive.rs \
        crates/missiond-mcp/src/tools/knowledge/plan.rs \
        crates/missiond-mcp/src/tools/knowledge/workflow.rs
git commit -m "feat(review): resolve review gates explicitly"
```

Only stage files actually modified.

## Report

Return:

- Commit hash.
- Final action/input contract.
- Review id parse contract.
- State transitions implemented.
- Tests and full acceptance results.
