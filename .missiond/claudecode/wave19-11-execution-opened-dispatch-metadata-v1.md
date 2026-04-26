# wave19-11-execution-opened-dispatch-metadata-v1 — ExecutionEvent Opened dispatch metadata v1

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave19/wave19-11-execution-opened-dispatch-metadata-v1.lisp`

## Machine Contract

- kind: `code-alignment`
- status: `ready`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- depends_on: `wave19-08-execution-task-contract-completion-v0`

## Goal

Extend the older ExecutionEvent::Opened path with dispatch metadata so consumers do not need companion log reads for the first lifecycle event.

## Ownership

- `crates/missiond-core/src/event/events/execution.rs`
- `crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs`

## Must Not Touch

- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`
- `crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs`
- `crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs`
- `.missiond/v2/*.lisp`
- `scripts/**`

## Requirements

1. Add optional dispatch_strategy, target_project, and requested_cwd fields to ExecutionEvent::Opened with serde defaults and skip_serializing_if so legacy JSON remains readable.
2. Wire mission_execution(open) to publish these fields when present.
3. Keep existing event ids and companion log behavior intact.
4. Add round-trip tests for old JSON without fields and new JSON with fields.

## Acceptance Commands

```bash
cargo test -p missiond-core --lib
cargo test -p missiond-daemon handlers::knowledge::agent_execution::tests
cargo test -p missiond-daemon
cargo build --workspace
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check -- crates/missiond-core/src/event/events/execution.rs crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs
```

## Commit

After acceptance, commit only files inside the declared write scope.

```bash
git add "crates/missiond-core/src/event/events/execution.rs" \
        "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
git commit -m "feat(execution): include dispatch metadata on opened events"
```

Scope check: `write-scope-only`.

## Report

- `Commit hash.`
- `Serde backward compatibility proof.`
- `Event publish wiring notes.`
- `Acceptance command results.`

