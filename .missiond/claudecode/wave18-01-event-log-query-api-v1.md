# Wave 18 Task 01 — Event Log Query API v1

## Goal

Turn the Wave17 event-ref persistent lookup into a reusable event-log query API.

Wave17 reused low-level log reading in the evidence path. This task adds a bounded, typed read API so future features can query events by domain/kind/correlation without reimplementing scans.

## Ownership

Expected files:

- `crates/missiond-core/src/db/traits.rs`
- `crates/missiond-core/src/db/pg/*`
- `crates/missiond-core/src/event/log/*`
- `crates/missiond-daemon/src/handlers/knowledge/evidence_collector.rs`
- `crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs`

Do not modify event enum wire format.

## Requirements

1. Add a read-only event query surface that can express:

   - domain
   - kind
   - optional correlation fields from JSON payload
   - since/until or bounded time window
   - limit with defensive cap

2. Reuse existing event log storage. Do not add a migration unless there is already a documented index/table that must be exposed.

3. Implement PG/store side only if the current store abstraction needs it. Prefer reusing existing projection/log reader APIs if possible.

4. Update evidence event-ref resolver to use this API instead of local ad hoc scan logic.

5. Failure behavior:

   - query error returns unavailable event ref with reason
   - primary dispatch/evidence write must not fail because event lookup failed

6. Add response/evidence metadata:

   - `event_ref_status`: `live | log | unavailable`
   - `event_ref_source`: `passive_cache | event_log_query | unavailable`
   - optional `event_ref_warning`

## Tests

Add tests for:

- query builder caps limit
- payload correlation exact match
- live cache wins over log
- event-log query returns ref after cache miss
- query error degrades to unavailable

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
git add crates/missiond-core/src/db/traits.rs \
        crates/missiond-core/src/db/pg \
        crates/missiond-core/src/event/log \
        crates/missiond-daemon/src/handlers/knowledge/evidence_collector.rs \
        crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs
git commit -m "feat(event): add bounded event log query API"
```

Only stage files actually modified.

## Report

Return:

- Commit hash.
- API shape.
- Whether a migration was avoided.
- Evidence resolver changes.
- Tests and acceptance results.
