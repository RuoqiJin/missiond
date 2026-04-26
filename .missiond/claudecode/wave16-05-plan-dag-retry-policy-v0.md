# Wave 16 Task 05 — PLAN DAG Retry Policy v0

## Goal

Implement bounded per-node retry for PLAN DAG runtime.

The full 11-stage scheduler is still larger than one task. This task carves out one contained slice: retry failed node dispatch according to explicit node policy, with attempt evidence.

## Dependency

Run after Wave16-04 if both are active, because both touch `plan_dag.rs`.

## Ownership

Expected files:

- `crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs`
- `crates/missiond-mcp/src/tools/knowledge/plan.rs`

Optional:

- `crates/missiond-daemon/src/handlers/knowledge/evidence_collector.rs` only if typed evidence helpers are needed.

Do not modify Lisp; Wave16 backfill is later.

## Requirements

1. Parse explicit retry hints:

   - `:retry-count <n>` or `:max-attempts <n>`
   - optional `:retry-delay-ms <n>`

2. Defaults:

   - no retry unless explicitly configured
   - cap attempts to a safe maximum, for example 3
   - reject negative / non-numeric values with structured parse error

3. Runtime:

   - On dispatch failure, retry until attempts exhausted.
   - Each attempt records evidence with attempt number and outcome.
   - Final node state is `succeeded` if any attempt succeeds, otherwise `failed`.
   - Respect existing `failure-policy` behavior after final failure.

4. Do not retry safe-descriptor refusals from workstation dispatch unless the refusal is explicitly retryable.

5. Dry-run must show retry plan but perform no dispatch.

## Tests

Add tests for:

- parser captures retry-count / max-attempts
- invalid retry values fail fast
- one failure then success marks succeeded
- exhausted retries marks failed
- dry-run response shows retry plan
- safe-descriptor non-retryable refusal not retried

## Acceptance Commands

```bash
cargo test -p missiond-daemon handlers::knowledge::plan_dag::tests
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
        crates/missiond-daemon/src/handlers/knowledge/evidence_collector.rs \
        crates/missiond-mcp/src/tools/knowledge/plan.rs
git commit -m "feat(plan): retry DAG node dispatch attempts"
```

Only stage files actually modified.

## Report

Return:

- Commit hash.
- Retry input contract.
- Attempt evidence shape.
- Failure-policy interaction.
- Tests and acceptance results.
