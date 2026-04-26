# wave19-08-execution-task-contract-completion-v0 — Execution task-contract completion verification v0

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave19/wave19-08-execution-task-contract-completion-v0.lisp`

## Machine Contract

- kind: `code-alignment`
- status: `ready`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- depends_on: `wave19-02-task-contract-verifier-v1`, `wave19-03-report-contract-v1`, `wave19-04-shared-memory-ledger-v0`

## Goal

Connect mission_execution completion metadata to task-contract verification results, without making the daemon run mutating git commands.

## Ownership

- `crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs`
- `crates/missiond-mcp/src/tools/knowledge/agent_execution.rs`

## Must Not Touch

- `crates/missiond-core/src/event/events/execution.rs`
- `crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs`
- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`
- `.missiond/v2/*.lisp`
- `scripts/**`

## Requirements

1. Add optional task_contract_path and task_report_path fields to mission_execution complete/preflight paths as metadata only.
2. Completion should record verifier_status-like fields supplied by caller or derived by read-only checks already present in daemon; do not shell out to mutating git commands.
3. If enforce_scoped_commit=true and task_contract_path is supplied, require commit_hash and ensure claimed scope is present; reject missing critical data with structured errors.
4. Preserve legacy complete behavior when new fields are absent.

## Acceptance Commands

```bash
cargo test -p missiond-daemon handlers::knowledge::agent_execution::tests
cargo test -p missiond-daemon
cargo test -p missiond-mcp --lib
cargo build --workspace
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check -- crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs crates/missiond-mcp/src/tools/knowledge/agent_execution.rs
```

## Commit

After acceptance, commit only files inside the declared write scope.

```bash
git add "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs" \
        "crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"
git commit -m "feat(execution): record task contract completion checks"
```

Scope check: `write-scope-only`.

## Report

- `Commit hash.`
- `New fields and enforcement conditions.`
- `Legacy compatibility notes.`
- `Acceptance command results.`

