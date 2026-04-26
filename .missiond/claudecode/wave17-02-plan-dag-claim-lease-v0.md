# Wave 17 Task 02 — PLAN DAG Claim / Lease v0

## Goal

Add claim/lease discipline to PLAN DAG node execution.

The full 11-stage scheduler calls for claim-lease coordination. This task implements a contained v0: each dispatching DAG node records a claim lifecycle tied to the existing mission_execution coordination model, without introducing a new lock service.

## Dependency

Run after Wave17-01 if both are active, because both touch `plan_dag.rs`.

## Ownership

Expected files:

- `crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs`
- `crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs` only if you need to reuse/expose pure claim helpers
- `crates/missiond-mcp/src/tools/knowledge/plan.rs`

Do not modify Lisp.

## Requirements

1. Parse optional scheduler args:

   - `claim_lease_secs` default 1800
   - `claimer_name` default `plan-dag-scheduler`
   - `enforce_claims` default false for compatibility

2. Per node dispatch lifecycle:

   - before dispatch: mark node `claimed`
   - then mark `running`
   - after dispatch: mark `succeeded` / `failed` / `paused` / `skipped`
   - release claim after terminal state

3. Reuse existing scope overlap semantics where possible.

   Node scope should derive from:

   - node `:owned-files`
   - node `:scope`
   - plan id + node id fallback

4. If `enforce_claims=true`:

   - refuse to dispatch when claim cannot be acquired
   - surface structured error/warning in node result

5. If `enforce_claims=false`:

   - record claim metadata best-effort, but do not break legacy DAG execution.

6. Evidence:

   - include claim id / claimer / lease_expires_at
   - include release timestamp

7. Dry-run:

   - show planned claims, no mutation.

## Tests

Add tests for:

- claim metadata generated from owned-files
- fallback scope from plan/node id
- dry-run shows planned claims
- enforce false preserves legacy dispatch
- enforce true refuses overlapping claim
- released claim permits later completion audit

## Acceptance Commands

```bash
cargo test -p missiond-daemon handlers::knowledge::plan_dag::tests
cargo test -p missiond-daemon handlers::knowledge::agent_execution::tests
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
        crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs \
        crates/missiond-mcp/src/tools/knowledge/plan.rs
git commit -m "feat(plan): add claim lease state to DAG nodes"
```

Only stage files actually modified.

## Report

Return:

- Commit hash.
- Claim scope derivation.
- Enforce/compat behavior.
- Evidence fields.
- Tests and acceptance results.
