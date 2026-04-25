# MissionD v2 Code Alignment Task: Execution / Capability Usage Event-Bus Extensions

请按 MissionD v2 Lisp 架构做代码同构：为已经落地的 `mission_execution` 和 `mission_capability_usage` 增补事件总线观测事件。

只做代码同构，不重新设计架构，不修改 `.missiond/v2/*.lisp`。当前 Lisp 是工作树里的最新设计，请先读这些锚点：

- `/Users/jinchen/Projects/missiond/.missiond/v2/intent-event-bus.lisp` :: `planned-event-extensions`
- `/Users/jinchen/Projects/missiond/.missiond/v2/intent-flow.lisp` :: `F-execution-log-governance`
- `/Users/jinchen/Projects/missiond/.missiond/v2/intent-flow.lisp` :: `F-capability-usage-monitoring`
- `/Users/jinchen/Projects/missiond/.missiond/v2/intent-memory.lisp` :: `agent-execution-coordination`
- `/Users/jinchen/Projects/missiond/.missiond/v2/intent-memory.lisp` :: `capability-usage-read-model`

## Parallel Scope

This task owns the event lane:

- `crates/missiond-core/src/event/**`
- daemon bus publish helper wiring
- `crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs`
- `crates/missiond-daemon/src/handlers/comm/capability_usage.rs`
- event tests

Do not touch slot cwd/project-root files. Avoid MCP registration files unless required by existing event type generation.

## Goals

1. Inspect the current event-bus implementation before editing:
   - domains / `Domain::ALL`
   - event enums
   - log persistence / serde shape
   - publish helper conventions
2. Implement execution observability for `mission_execution`:
   - preferred Lisp candidate: `ExecutionEvent`
   - candidate variants: `Opened`, `Claimed`, `Heartbeat`, `Released`, `DeviationRecorded`, `DecisionRecorded`, `IssueRecorded`, `Completed`, `Audited`, `Repaired`, `StaleClaim`
   - if adding a new `Domain::Execution` has broad blast radius, fold into existing `SystemEvent` or another already suitable domain, but document why in code comments/report
3. Implement capability usage observability:
   - preferred placement: extend existing `ObservabilityEvent`
   - candidate variants: `CapabilityUsageSnapshot`, `CapabilityStaleCandidate`
   - publish from `mission_capability_usage(action=snapshot|report|candidates)` only after snapshot/candidate computation succeeds
4. Preserve durable evidence ownership:
   - execution companion Lisp files remain memory/file truth
   - capability usage source data remains tool audit / read-model truth
   - bus event is live projection / notification, not new storage truth
5. Keep backward-compatible serde/wire behavior where current tests require it.

## Non-Goals

- Do not redesign the 12-domain event-bus model beyond the smallest needed extension.
- Do not change `.missiond/v2/*.lisp`.
- Do not reimplement `mission_execution` or `mission_capability_usage` handlers.
- Do not add destructive behavior to capability usage.

## Acceptance

- `cargo build --workspace`
- `cargo test -p missiond-core --lib`
- `cargo test -p missiond-daemon`
- `node scripts/check-architecture-lisp.mjs --all-v2`
- `git diff --check`

Add focused tests where practical:

- event serialization/deserialization for new variants
- publish helper routes to expected domain
- `mission_execution` action emits the expected event after successful mutation
- `mission_capability_usage` snapshot/candidates emits observability event without altering review state

## Deliverables

- List modified files.
- State final placement decision: new `Domain::Execution` vs folded domain.
- State which events are fully emitted and which remain TODO.
- State any compatibility risk.
