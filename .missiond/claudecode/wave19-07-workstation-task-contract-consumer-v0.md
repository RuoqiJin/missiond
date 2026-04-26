# wave19-07-workstation-task-contract-consumer-v0 — Workstation task-contract consumer v0

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave19/wave19-07-workstation-task-contract-consumer-v0.lisp`

## Machine Contract

- kind: `code-alignment`
- status: `ready`
- owner: `claudecode`
- dispatch_strategy: `agent-team`
- depends_on: `wave19-05-renderer-dispatch-brief-v1`

## Goal

Teach workstation_dispatch to prefer a task.lisp contract when one exists, and render a ClaudeCode brief from contract fields rather than re-inventing natural-language instructions.

## Ownership

- `crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs`

## Must Not Touch

- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`
- `crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs`
- `crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs`
- `.missiond/v2/*.lisp`
- `scripts/**`

## Requirements

1. Use agent-team if useful: 使用 agent-team提高效率.
2. Add a pure parser/loader for the narrow task-contract v1 fields needed by workstation_dispatch, or reuse existing in-Rust Lisp helpers if present.
3. Accept optional task_contract_path in the internal descriptor; when present, build the task brief from the contract and preserve existing scoped-commit handoff section.
4. Keep legacy objective/owned-files path unchanged when task_contract_path is absent.
5. If contract is malformed, return SafeDescriptor-style structured failure and do not fall back to claude -p.

## Acceptance Commands

```bash
cargo test -p missiond-daemon handlers::knowledge::workstation_dispatch::tests
cargo test -p missiond-daemon
cargo build --workspace
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check -- crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs
```

## Commit

After acceptance, commit only files inside the declared write scope.

```bash
git add "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
git commit -m "feat(workstation): consume Lisp task contracts"
```

Scope check: `write-scope-only`.

## Report

- `Commit hash.`
- `Contract fields consumed.`
- `Malformed-contract behavior.`
- `Compatibility boundaries.`
- `Acceptance command results.`

