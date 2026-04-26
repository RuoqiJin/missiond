# Wave 16 Task 06 — Scoped Commit Enforce v0

## Goal

Move scoped commit from audit-only toward enforceable runtime policy.

Current `mission_execution(action=complete)` records `changed_files`, `staged_files`, `commit_hash`, and `commit_status`, and audit reports scope violations. This task adds opt-in fail-fast enforcement at completion time.

Do not run git commands inside the daemon. The daemon validates caller-reported metadata against active/released claims.

## Ownership

Expected files:

- `crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs`
- `crates/missiond-mcp/src/tools/knowledge/agent_execution.rs`

Do not modify plan/workstation code in this task.

## Requirements

1. Add optional input:

   - `enforce_scoped_commit` boolean, default false for backward compatibility

2. When `enforce_scoped_commit=true` and `action=complete`:

   - `commit_status="committed"` requires non-empty `commit_hash`
   - `commit_status="blocked"` requires non-empty `commit_blocker`
   - if `staged_files` is non-empty, every staged path must overlap an active or released claim scope for the same execution
   - if no claims exist and staged_files is non-empty, reject

3. Reuse existing audit helper logic where possible. Do not duplicate scope-overlap rules.

4. Return structured errors:

   - `COMMIT_HASH_REQUIRED`
   - `COMMIT_BLOCKER_REQUIRED`
   - `SCOPED_COMMIT_VIOLATION`
   - `CLAIM_SCOPE_REQUIRED`

5. Legacy behavior:

   - With `enforce_scoped_commit` absent/false, keep current audit-only behavior.

6. Response:

   - include `scoped_commit_enforced: true|false`
   - include `scoped_commit_validation` on success

## Tests

Add tests for:

- enforce false keeps legacy completion accepted
- committed without hash rejected when enforce true
- blocked without blocker rejected when enforce true
- staged file outside claim rejected
- staged file inside released claim accepted
- no claims + staged files rejected

## Acceptance Commands

```bash
cargo test -p missiond-daemon handlers::knowledge::agent_execution::tests
cargo test -p missiond-daemon
cargo test -p missiond-mcp --lib
cargo build --workspace
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check
```

## Commit

```bash
git add crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs \
        crates/missiond-mcp/src/tools/knowledge/agent_execution.rs
git commit -m "feat(execution): enforce scoped commit handoff on request"
```

## Report

Return:

- Commit hash.
- Enforcement contract.
- Backward-compat behavior.
- Tests and acceptance results.
