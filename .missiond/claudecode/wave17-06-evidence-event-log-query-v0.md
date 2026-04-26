# Wave 17 Task 06 — Evidence Event-Log Query v0

## Goal

Upgrade Wave16 evidence event refs from passive in-memory cache only to cache + persistent event-log query.

Wave16 added `live/log/unavailable` status language but mainly relies on a passive cache. This task implements the persistent query path so event refs survive daemon restarts.

## Ownership

Expected files:

- `crates/missiond-daemon/src/handlers/knowledge/evidence_collector.rs`
- `crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs`
- `crates/missiond-core/src/db/traits.rs` only if a store method is needed
- `crates/missiond-core/src/db/pg/*` only if implementing a new store method

Do not modify event enum wire format.

## Requirements

1. Add a resolver strategy:

   1. live passive cache
   2. persistent event log query
   3. unavailable fallback

2. Query by deterministic correlation fields:

   - domain `execution`
   - kind `plan_node_state_changed`
   - plan_id
   - node_id
   - optional attempt
   - optional transition from/to
   - bounded time window or limit

3. If the existing store already has a generic event query method, reuse it.

4. If adding a store method:

   - keep it read-only
   - no migration unless absolutely necessary
   - limit rows defensively

5. Failure behavior:

   - query failure -> unavailable with reason
   - primary dispatch/evidence write must not fail because event lookup failed

6. Response/evidence should surface:

   - `event_ref_status`: `live | log | unavailable`
   - `event_ref_source`
   - `event_ref_warning` when applicable

## Tests

Add tests for:

- live cache wins over log
- log query returns ref after cache miss
- no match -> unavailable
- query error -> unavailable warning
- plan_dag evidence includes log source when found

## Acceptance Commands

```bash
cargo test -p missiond-core --lib
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
        crates/missiond-core/src/db/traits.rs \
        crates/missiond-core/src/db/pg
git commit -m "feat(evidence): resolve event refs from event log"
```

Only stage files actually modified.

## Report

Return:

- Commit hash.
- Store/query method used.
- Resolver precedence.
- Tests and acceptance results.
