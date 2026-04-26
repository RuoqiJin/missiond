# Wave 16 Task 02 — Review Gate QuestionEvent Listener v0

## Goal

Move review-gate resolution from pure caller-push toward event-loop behavior.

Wave14 emits `QuestionEvent::Created`. Wave15/16 explicit resolution consumes `review_question_id` inputs. This task adds a conservative subscriber for `QuestionEvent::Resolved` that recognizes deterministic `review:*` question ids and routes them through the existing explicit resolution bridge.

This is still not autonomous approval from arbitrary text. It only reacts to deterministic review ids and recognized resolution strings.

## Dependency

Run after:

- Wave16-01 workflow review resolution v0

## Ownership

Expected files:

- `crates/missiond-daemon/src/bus/v2_subscribers.rs`
- `crates/missiond-daemon/src/handlers/knowledge/review_gate.rs`
- `crates/missiond-daemon/src/handlers/knowledge/directive.rs`
- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`
- `crates/missiond-daemon/src/handlers/knowledge/workflow.rs`

Do not modify MCP schemas unless you add a user-facing action/field, which should not be necessary.

Do not modify event enum wire format.

## Requirements

1. Add a subscriber path for `QuestionEvent::Resolved`.

   Existing decision subscriber handles `QuestionEvent::Created`. Extend or add a focused subscriber without breaking the existing decision engine path.

2. Only handle ids matching deterministic review-gate envelope:

   ```text
   review:<scope>:<artifact_id>:v<version>:<action>[:<topic-hash>]
   ```

   Non-review question ids must be ignored after ack.

3. Map `resolution` string conservatively:

   - `approved`, `approve`, `yes`, `accepted` -> `approved`
   - `rejected`, `reject`, `no` -> `rejected`
   - `needs_changes`, `changes`, `revise`, `fix` -> `needs_changes`
   - anything else -> no mutation; emit/log structured warning

4. Route through the same validation logic as explicit resolution.

   Do not duplicate state-transition rules in the subscriber. Prefer a shared helper in `review_gate.rs` or handler-local public(crate) helpers.

5. Subscriber errors:

   - Ack the bus message after handling attempt.
   - Log warning and record observability if available.
   - Do not panic.

6. No auto-approve for non-review questions.

## Tests

Add pure tests for resolution string mapping.

Add handler/subscriber helper tests for:

- non-review id ignored
- approved directive id routes to approved bridge
- rejected plan id routes to rejected bridge
- workflow id routes after Wave16-01
- unknown resolution returns ignored/warning

If direct subscriber tests require too much runtime setup, extract a pure `ReviewResolvedDispatch` planner and test that.

## Acceptance Commands

```bash
cargo test -p missiond-daemon handlers::knowledge::review_gate::tests
cargo test -p missiond-daemon bus::v2_subscribers::tests
cargo test -p missiond-daemon handlers::knowledge::directive::tests
cargo test -p missiond-daemon handlers::knowledge::plan::tests
cargo test -p missiond-daemon handlers::knowledge::workflow::tests
cargo test -p missiond-daemon
cargo build --workspace
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check
```

If `bus::v2_subscribers::tests` does not exist, run the nearest targeted tests you add and state the exact module path.

## Commit

```bash
git add crates/missiond-daemon/src/bus/v2_subscribers.rs \
        crates/missiond-daemon/src/handlers/knowledge/review_gate.rs \
        crates/missiond-daemon/src/handlers/knowledge/directive.rs \
        crates/missiond-daemon/src/handlers/knowledge/plan.rs \
        crates/missiond-daemon/src/handlers/knowledge/workflow.rs
git commit -m "feat(review): consume review question resolutions"
```

Only stage files actually modified.

## Report

Return:

- Commit hash.
- Subscriber behavior.
- Resolution string mapping.
- Exact files changed.
- Tests and acceptance results.
