# Wave 16 Task 07 — Evidence Event Subscription v0

## Goal

Start replacing `EventRef::unavailable` placeholders with real event references from the event bus / event log where possible.

Wave14/15 publish execution and plan-node events. Evidence collector can already store event refs. This task adds a conservative resolver/subscriber path so sidecar evidence can attach real event ids when available.

## Ownership

Expected files:

- `crates/missiond-daemon/src/handlers/knowledge/evidence_collector.rs`
- `crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs`
- `crates/missiond-daemon/src/bus/v2_subscribers.rs` only if you add a live subscriber

Optional:

- `crates/missiond-daemon/src/bus/bootstrap.rs` only if a helper is needed

Do not modify event enum wire format in this task.

## Requirements

Choose the lowest-risk implementation that can produce real refs:

1. Prefer querying existing event log by deterministic correlation fields if that is already available.
2. If query support is not available, add a passive live subscriber cache with bounded in-memory retention.
3. Keep fallback behavior:

   - live/log match found -> `EventRef::new(domain, kind, event_id)`
   - no match -> `EventRef::unavailable(reason)`

4. Correlation keys:

   - plan id
   - node id
   - execution id
   - event kind
   - generated/recorded timestamp window if available

5. No failure in event lookup may fail primary dispatch or evidence write.

6. Response should surface whether refs were `live`, `log`, or `unavailable`.

## Tests

Add tests for:

- resolver returns real ref from synthetic cache/log fixture
- resolver returns unavailable when no match
- plan_dag evidence includes ref status
- lookup failure degrades to unavailable with reason

## Acceptance Commands

```bash
cargo test -p missiond-daemon handlers::knowledge::evidence_collector::tests
cargo test -p missiond-daemon handlers::knowledge::plan_dag::tests
cargo test -p missiond-daemon
cargo build --workspace
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check
```

## Commit

```bash
git add crates/missiond-daemon/src/handlers/knowledge/evidence_collector.rs \
        crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs \
        crates/missiond-daemon/src/bus/v2_subscribers.rs \
        crates/missiond-daemon/src/bus/bootstrap.rs
git commit -m "feat(evidence): attach live event refs when available"
```

Only stage files actually modified.

## Report

Return:

- Commit hash.
- Resolver strategy chosen.
- Fallback behavior.
- Tests and acceptance results.
