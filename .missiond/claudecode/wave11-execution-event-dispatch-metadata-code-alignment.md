# MissionD wave 11 code-alignment: ExecutionEvent dispatch metadata

使用 agent-team 提高效率，但写入范围必须保持清晰。只做代码向 Lisp 对齐，不重新设计架构，不修改 `.missiond/v2/*.lisp`，不 stage，不 commit。

## 目标

`mission_execution(action=open)` 已把 `dispatch_strategy / target_project / requested_cwd` 写入 companion log meta。当前 `ExecutionEvent::Opened` 还没有这些字段。本任务做 additive event metadata 扩展，让 live bus projection 也带同样信息。

## Lisp 锚点

- `.missiond/v2/intent-worker.lisp :: claudecode-workstation-orchestration :: execution-strategy-record`
- `.missiond/v2/intent-flow.lisp :: F-workstation-dispatch-policy :: s4 record-strategy`
- `.missiond/v2/intent-event-bus.lisp :: planned-event-extensions :: ExecutionEvent`

## 写入范围

允许修改：

- `crates/missiond-core/src/event/events/execution.rs`
- `crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs`
- `crates/missiond-mcp/src/tools/knowledge/agent_execution.rs` only if schema docs need fields clarified

不要修改：

- `plan.rs`
- `workflow.rs`
- DB migrations
- `.missiond/v2/*.lisp`

## 行为要求

Extend `ExecutionEvent::Opened` additively with optional fields:

- `dispatch_strategy: Option<String>`
- `target_project: Option<String>`
- `requested_cwd: Option<String>`

Open handler:

- Extract the normalized values already used for companion log meta.
- Emit them in `ExecutionEvent::Opened`.
- Existing callers that do not pass these fields still serialize/deserialize cleanly.

Compatibility:

- If Rust enum serde is externally tagged, adding fields to struct variant is additive for producers but old consumers may ignore at JSON layer; preserve old required fields.
- Tests must include old JSON without fields if feasible, or at least new round-trip + old construction path.

## Tests

At minimum:

- ExecutionEvent::Opened round-trip with metadata.
- ExecutionEvent::Opened round-trip without metadata / construction still works.
- agent_execution open emit helper includes metadata when args present.

## 验收

- `cargo test -p missiond-core --lib`
- `cargo test -p missiond-daemon handlers::knowledge::agent_execution::tests`
- `cargo test -p missiond-daemon`
- `cargo build --workspace`
- `node scripts/check-architecture-lisp.mjs --all-v2`
- `git diff --check`

## 交付报告

说明：

- Changed event fields.
- Old event compatibility.
- Which open args map to event metadata.

