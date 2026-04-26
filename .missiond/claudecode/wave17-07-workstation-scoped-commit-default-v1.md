# Wave 17 Task 07 — Workstation Scoped-Commit Default v1

## Goal

Make scoped-commit enforcement the default expectation for workstation-generated code tasks, without making global mission_execution backward-incompatible.

Wave16 added opt-in `enforce_scoped_commit`. This task makes workstation dispatch briefs and generated completion instructions require the opt-in flag by default.

The daemon still must not run git.

## Ownership

Expected files:

- `crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs`
- `crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs` only if a helper/response field is needed
- `crates/missiond-mcp/src/tools/knowledge/plan.rs`
- `crates/missiond-mcp/src/tools/knowledge/agent_execution.rs` only if schema wording needs updating

Do not modify Lisp.

## Requirements

1. Workstation task brief must instruct the worker to call/report completion with:

   - `enforce_scoped_commit=true`
   - `commit_status`
   - `commit_hash` when committed
   - `staged_files`

2. If generated task is read-only, brief may set `commit_status=not-required` but must explain why.

3. Add response fields from workstation dispatch:

   - `scoped_commit_required: true`
   - `scoped_commit_policy: "enforced-on-complete"`

4. Do not make `mission_execution(action=complete)` enforce by default for all legacy callers.

5. Do not install git hooks in this task. If hooks remain desired, list as future work.

6. Agent-team hint still appears exactly once.

## Tests

Add tests for:

- generated code task brief includes `enforce_scoped_commit=true`
- read-only task brief uses `not-required`
- response includes scoped_commit_required
- legacy mission_execution complete default unchanged
- agent-team hint still exactly once

## Acceptance Commands

```bash
cargo test -p missiond-daemon handlers::knowledge::workstation_dispatch::tests
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
git add crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs \
        crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs \
        crates/missiond-mcp/src/tools/knowledge/plan.rs \
        crates/missiond-mcp/src/tools/knowledge/agent_execution.rs
git commit -m "feat(workstation): require scoped commit handoff in task briefs"
```

Only stage files actually modified.

## Report

Return:

- Commit hash.
- Brief contract.
- Backward compatibility statement.
- Tests and acceptance results.
