# MissionD v2 code-alignment: plan-runner auto-selection v1

使用 agent-team 提高效率，但写入范围必须保持清晰。只做代码向 Lisp 对齐，不重新设计架构，不修改 `.missiond/v2/*.lisp`，不 stage，不 commit。

## 目标

把 `mission_plan(action=execute)` 从“caller 必须显式传 target/dispatch_strategy”推进到 plan-runner auto-selection v1：

- `target` 仍可显式传入并优先。
- 当 `target` 缺省时，runner 从 `plan.sexp_text` 中保守提取 target / flow_id / dispatch_strategy / parallelism / target_project / requested_cwd。
- 能把 PLAN.lisp 节点里的 workstation hints 转换成当前已有 substrate：
  - `mission_execution`
  - `mission_task_delegate`
  - `mission_flow_run`
- 不新增 MCP tool，不新增 migration，不改 Lisp。

Lisp 锚点：

- `.missiond/v2/intent-flow.lisp :: F-intent-alignment-plan-execution-loop :: s6 execution-runner`
- `.missiond/v2/intent-flow.lisp :: F-workstation-dispatch-policy`
- `.missiond/v2/intent-intent-layer.lisp :: unified-entry-pipeline :: role plan-runner`
- `.missiond/v2/intent-worker.lisp :: claudecode-workstation-orchestration`
- `.missiond/v2/intent-tools.lisp :: mission_plan :: :dispatch-strategy-consumer`

## 写入范围

主要 ownership：

- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`
- `crates/missiond-mcp/src/tools/knowledge/plan.rs`

不要改：

- `.missiond/v2/*.lisp`
- `directive.rs`
- `workflow.rs`
- `agent_execution.rs`
- DB migrations

## 期望行为

### 1. Backward compatibility

现有行为必须保留：

- 显式 `target` 的调用按现有逻辑执行。
- `execute_mode="bridge"` 仍返回 next_call。
- `execute_mode="internal"` 仍直接 dispatch。
- `dry_run=true` 不产生 side effect。
- 非 approved/executing plan 仍拒绝。

### 2. Auto-selection parser

新增一个小的保守 parser，不需要完整 Lisp 解释器。只解析明确 key/value hints：

- `:target` / `:target-tool` / `:tool`
- `:flow-id` / `:flow_id`
- `:dispatch-strategy` / `:dispatch_strategy`
- `:parallelism`
- `:target-project` / `:target_project` / `:project`
- `:requested-cwd` / `:requested_cwd` / `:cwd`
- `:objective` / `:summary`

Allowed target mapping:

- string contains `mission_execution` or `execution` => `mission_execution`
- string contains `mission_task_delegate` / `task_delegate` / `claudecode` / `code-alignment` => `mission_task_delegate`
- string contains `mission_flow_run` / `flow_run` / `flow` and has `flow_id` => `mission_flow_run`

If parser cannot derive a safe target:

- keep current structured `MISSING_PARAM` behavior,
- but suggestion should mention adding `target` or PLAN hint fields.

### 3. Dispatch strategy mapping

Normalize hints to existing enum:

- `resident-lisp`
- `fresh-code-alignment`
- `agent-team`
- `mixed`
- `prompt-fallback`
- `unknown`

Rules:

- explicit `dispatch_strategy` arg wins.
- explicit plan hint wins over default.
- `parallelism="agent-team"` or text containing `agent-team` maps to `agent-team`.
- code-alignment / fresh session hints map to `fresh-code-alignment`.
- lisp / architecture / resident hints map to `resident-lisp`.
- unknown maps to `unknown`, never hard-fails.

### 4. Build inner args from parsed plan hints

When caller omitted fields but parser found them:

- `mission_execution`: pass `dispatch_strategy`, `target_project`, `requested_cwd` as already supported.
- `mission_task_delegate`: derive `objective`; if `dispatch_strategy=agent-team`, include the literal hint `使用 agent-team提高效率` in the delegated objective or a supported notes field, without duplicating if already present.
- `mission_flow_run`: pass `flow_id` and `params` if discovered / supplied.

The response should include:

- `target_source`: `"explicit_arg" | "plan_hint" | "missing"`
- `dispatch_strategy_source`: `"explicit_arg" | "plan_hint" | "default"`
- `plan_hint_summary`: compact object with extracted fields, in bridge/dry_run/internal success responses.

### 5. Tests

Add pure-function tests:

- extracts target/dispatch_strategy/flow_id/project/cwd from simple PLAN snippets.
- explicit args override plan hints.
- `parallelism=agent-team` maps to dispatch_strategy agent-team and adds task objective hint.
- missing target with no hints keeps structured MISSING_PARAM.
- bridge response includes `target_source` / `plan_hint_summary`.
- internal mission_execution args include parsed dispatch metadata.
- internal mission_flow_run accepts parsed flow_id.

Run acceptance:

```bash
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
- auto-selection 支持的 hint 字段
- target / dispatch_strategy source 优先级
- agent-team hint 如何落到 task_delegate
- 保持兼容的行为
- 测试结果
- 明确说明未修改 `.missiond/v2/*.lisp`、未 stage、未 commit
