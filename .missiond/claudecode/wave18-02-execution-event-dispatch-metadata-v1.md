# Wave 18 Task 02 — ExecutionEvent Dispatch Metadata v1

## Goal

Finish the pending ExecutionEvent dispatch metadata expansion.

Companion logs already persist `dispatch_strategy`, `target_project`, and `requested_cwd`. PlanNodeStateChanged also carries dispatch metadata. This task extends the relevant ExecutionEvent variants so event consumers no longer need to read companion logs for basic dispatch context.

## Ownership

Expected files:

- `crates/missiond-core/src/event/events/execution.rs`
- `crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs`
- `crates/missiond-daemon/src/bus/bootstrap.rs` only if helper signatures need updating
- tests touching execution events

Do not modify plan DAG behavior in this task.

## Requirements

1. Extend additive event payloads with optional fields:

   - `dispatch_strategy`
   - `target_project`
   - `requested_cwd`

2. At minimum, cover:

   - `ExecutionEvent::Opened`
   - `ExecutionEvent::Claimed`
   - `ExecutionEvent::Completed`

   If other variants clearly carry dispatch context, include them only if low risk.

3. Backward compatibility:

   - serde default/skip absent fields
   - old JSON must still deserialize
   - new fields absent when not known

4. Emit path:

   - `mission_execution(open)` should include metadata when args provide it.
   - later events should read metadata from companion log where practical.

5. Do not require callers to pass metadata again after open.

## Tests

Add tests for:

- old JSON deserializes without metadata
- new JSON round-trips metadata
- open emits metadata
- completed event can inherit metadata from companion log or omits cleanly when unavailable

## Acceptance Commands

```bash
cargo test -p missiond-core --lib
cargo test -p missiond-daemon handlers::knowledge::agent_execution::tests
cargo test -p missiond-daemon
cargo build --workspace
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check
```

## Commit

```bash
git add crates/missiond-core/src/event/events/execution.rs \
        crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs \
        crates/missiond-daemon/src/bus/bootstrap.rs
git commit -m "feat(execution): include dispatch metadata in events"
```

Only stage files actually modified.

## Report

Return:

- Commit hash.
- Variants extended.
- Backward compatibility proof.
- Tests and acceptance results.
