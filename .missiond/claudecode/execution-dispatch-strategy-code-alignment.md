# MissionD v2 code-alignment: execution dispatch_strategy companion record

使用 agent-team 提高效率，但写入范围必须保持清晰。只做代码向 Lisp 对齐，不重新设计架构，不修改 `.missiond/v2/*.lisp`，不 stage，不 commit。

## 目标

把 unified-entry / workstation orchestration 里已经设计好的 `dispatch_strategy` 真正落到 `mission_execution` companion log：

- `mission_execution(action=open)` 接收并持久化 `dispatch_strategy`。
- `mission_plan(action=execute, execute_mode=internal, target=mission_execution)` 转发 `dispatch_strategy` 给 `mission_execution(action=open)`。
- `list/status/audit` 能读出该字段，legacy execution 文件缺字段时不报错。
- 不新增 MCP tool，不新增 migration，不改 Lisp。

Lisp 锚点：

- `.missiond/v2/intent-tools.lisp :: implemented-surface mission_execution :: :workstation-dispatch-record`
- `.missiond/v2/intent-tools.lisp :: implemented-surface mission_plan :: :dispatch-strategy-consumer`
- `.missiond/v2/intent-flow.lisp :: F-workstation-dispatch-policy`
- `.missiond/v2/intent-worker.lisp :: section claudecode-workstation-orchestration`

## 写入范围

主要 ownership：

- `crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs`
- `crates/missiond-mcp/src/tools/knowledge/agent_execution.rs`
- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`
- `crates/missiond-mcp/src/tools/knowledge/plan.rs`

不要改：

- `.missiond/v2/*.lisp`
- `directive.rs`
- `workflow.rs`
- DB migrations
- event enum/domain，除非现有 ExecutionEvent::Opened 已经有通用 metadata slot；没有就只留 TODO 注释，不扩大事件协议。

## 期望行为

### 1. mission_execution 参数扩展

给 `mission_execution(action=open)` schema 和 handler 增加可选参数：

- `dispatch_strategy`: enum-ish string, allowed known values:
  - `resident-lisp`
  - `fresh-code-alignment`
  - `agent-team`
  - `mixed`
  - `prompt-fallback`
  - `unknown`
- `target_project`: optional string, if existing code already uses `project` then keep `project` as canonical and treat `target_project` as alias.
- `requested_cwd`: optional string metadata only, if present.

Validation:

- Unknown / empty `dispatch_strategy` must normalize to `"unknown"` or return a structured warning field; do not hard-fail old callers.
- Existing callers with no field must keep working and write `"unknown"` or omit the field in a legacy-compatible way.

### 2. Companion log shape

For newly opened execution logs, persist dispatch metadata under the execution meta/header area, for example:

```lisp
:dispatch-strategy "fresh-code-alignment"
:target-project "missiond"
:requested-cwd "/Users/jinchen/Projects/missiond/crates/..."
```

Use the local file style already used by `agent_execution.rs`; do not invent a new top-level file shape.

Legacy files:

- `list`, `status`, `audit`, and `repair` must tolerate missing dispatch fields.
- Audit may emit informational warning for missing dispatch_strategy only on files whose meta says they are produced after this version; do not make old pilot files fail.

### 3. Plan-runner forwarding

In `mission_plan(action=execute)`:

- Bridge mode already includes `dispatch_strategy` in response; keep it.
- Internal `target=mission_execution` must include `dispatch_strategy` in the inner JSON passed to `agent_execution::handle`.
- If caller did not pass `dispatch_strategy`, derive the same normalized default currently used by execute response.
- Add/update tests around `build_internal_dispatch_args`.

### 4. Response surface

Update responses where useful:

- `open` response includes `dispatch_strategy`.
- `status` response includes it if present.
- `list` rows include it if cheaply available.
- `audit` summary may count missing/unknown dispatch strategies but must not fail legacy logs.

## 测试要求

Add focused unit tests only; no integration DB needed.

Required coverage:

- dispatch_strategy normalization.
- `mission_execution(open)` template contains dispatch metadata.
- legacy/missing dispatch_strategy parse remains OK.
- plan-runner internal `mission_execution` args include dispatch_strategy.
- MCP schema exposes the optional fields.

Run acceptance:

```bash
cargo test -p missiond-daemon handlers::knowledge::agent_execution::tests
cargo test -p missiond-daemon handlers::knowledge::plan::tests
cargo test -p missiond-daemon
cargo test -p missiond-mcp --lib
cargo build --workspace
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check
```

## 交付报告

请列：

- 修改文件
- `dispatch_strategy` 在 open/list/status/audit/plan-runner forwarding 的状态
- legacy execution 文件兼容性
- 是否触碰事件总线协议
- 测试结果
- 明确说明未修改 `.missiond/v2/*.lisp`、未 stage、未 commit
