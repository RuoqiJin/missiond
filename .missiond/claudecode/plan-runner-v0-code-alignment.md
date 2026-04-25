# MissionD v2 代码同构任务：mission_plan execute → plan-runner v0

工作目录：`/Users/jinchen/Projects/missiond`

你是 ClaudeCode。请做代码向 Lisp 对齐，不重新设计架构。

可以使用 agent-team 提高效率，但最终由主 agent 统一落笔，避免多人同时改同一 handler/schema。

## 背景

当前 Lisp 已经把 MissionD canonical unified entry pipeline 设计为：

```text
message
  -> intent-alignment.lisp
  -> review
  -> PLAN.lisp
  -> review
  -> MissionD internal execution
  -> evidence
  -> workflow.lisp
```

相关 Lisp 锚点：

- `.missiond/v2/intent-flow.lisp`
  - `F-intent-alignment-plan-execution-loop :: s6 execution-runner`
  - `F-workstation-dispatch-policy`
- `.missiond/v2/intent-intent-layer.lisp`
  - `section unified-entry-pipeline :: role plan-runner`
  - `workstation-dispatch-policy`
- `.missiond/v2/intent-tools.lisp`
  - `implemented-surface mission_plan :: :execute-contract`
  - `implemented-surface mission_execution :: :workstation-dispatch-record`
- `.missiond/v2/intent-worker.lisp`
  - `section claudecode-workstation-orchestration`

当前代码状态：

- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`
  - `action_execute` 目前只返回 `next_call` descriptor。
  - 不递归 dispatch。
- `crates/missiond-mcp/src/tools/knowledge/plan.rs`
  - schema 说明也写着 execute 仅返回 next_call。

这与 Lisp 的最终方向有差距：未来应由 MissionD `plan-runner` 内部消费 `mission_execution` / `mission_task_delegate` / `mission_flow_run`，而不是让 client 私有解析 `next_call`。

## 目标

实现 `mission_plan(action=execute)` 的 plan-runner v0。

必须保持向后兼容：

- 默认行为仍可返回 bridge descriptor。
- 只有显式请求 `execute_mode="internal"` 时才内部 dispatch。

建议 action contract：

```json
{
  "action": "execute",
  "plan_id": "...",
  "target": "mission_execution|mission_task_delegate|mission_flow_run",
  "execute_mode": "bridge|internal",
  "dispatch_strategy": "resident-lisp|fresh-code-alignment|agent-team|mixed|prompt-fallback|unknown",
  "target_project": "/path/or/project-id optional",
  "objective": "optional for task_delegate",
  "flow_id": "required only for mission_flow_run internal",
  "dry_run": false
}
```

Default:

- `execute_mode` defaults to `"bridge"` for backward compatibility.
- `dispatch_strategy` defaults to `"unknown"` if omitted.

## Required behavior

### 1. `execute_mode="bridge"`

Keep existing behavior, but enrich the response:

- Include `execute_mode: "bridge"`.
- Include `dispatch_strategy`.
- Include `runner_status: "bridge_only"`.
- Keep `next_call` exactly usable.
- Do not update plan status to executing.
- Do not internally dispatch anything.

### 2. `execute_mode="internal"` + `target="mission_execution"`

Internally dispatch `mission_execution(action=open)` via existing handler dispatch path or direct `agent_execution::handle`.

Expected:

- Validate plan status is `approved` or `executing`.
- Build execution id `plan-<plan_id>` unless caller supplies `execution_id`.
- Scope should include plan id + board_task_id.
- Include `dispatch_strategy` in the result payload even if `mission_execution` schema cannot persist it yet.
- Mark plan status to `executing` after successful dispatch.
- Return a structured result:

```json
{
  "status": "executing",
  "execute_mode": "internal",
  "target_tool": "mission_execution",
  "plan_id": "...",
  "board_task_id": "...",
  "dispatch_strategy": "...",
  "inner_result": ...
}
```

Do not fail the whole action if future event emission is unsupported; this is a manager action.

### 3. `execute_mode="internal"` + `target="mission_task_delegate"`

Internally dispatch `mission_task_delegate`.

Use existing handler path if possible:

- `objective` should come from args if provided.
- If absent, derive objective from plan row:
  - first line/short summary of `plan.sexp_text`, capped to a reasonable length.
  - include plan id in objective.
- Pass `cwd` or `target_project` if provided.
- Pass `intent`:
  - default `"code"`, unless caller supplies one of the valid values.
- Mark plan status to `executing` after successful dispatch.
- Return `inner_result`.

If `target_project`/`cwd` cannot resolve, allow existing task_delegate/project-root validation to surface structured error.

### 4. `execute_mode="internal"` + `target="mission_flow_run"`

Be conservative.

Internal dispatch is allowed only when caller supplies `flow_id`.

- If `flow_id` missing: return `MISSING_PARAM` or `INVALID_PARAM` with suggestion.
- If present: internally dispatch `mission_flow_run(action=run, flow_id, params?)`.
- Mark plan status to `executing` only after successful dispatch.

Do not invent automatic PLAN.lisp → YAML compilation in this task.

### 5. Evidence sidecar

After successful internal dispatch, append an execution evidence entry using existing sidecar writer logic or a small shared helper.

Evidence should include:

- `kind: "plan_runner_dispatch"`
- `execute_mode`
- `target_tool`
- `dispatch_strategy`
- `inner_result` or concise receipt
- `recorded_at`

Do not duplicate large blobs; avoid dumping huge text into evidence.

### 6. Dispatch strategy recording

Lisp says `mission_execution` schema/persistence of `dispatch_strategy` is future.

For this task:

- Do not change mission_execution companion-log schema unless clearly low-risk.
- Include `dispatch_strategy` in `mission_plan(action=execute)` response and evidence sidecar.
- If you add support in mission_execution as additive optional field, mark it clearly in report and update tests.

### 7. MCP schema

Update `crates/missiond-mcp/src/tools/knowledge/plan.rs`:

- Add optional `execute_mode`.
- Add optional `dispatch_strategy`.
- Add optional `target_project` or `cwd` if needed.
- Add optional `objective`, `intent`, `flow_id`, `params`.
- Update description: bridge is default; internal dispatch is plan-runner v0.

### 8. Lisp comments / TODOs

Do not redesign Lisp.

If code behavior still falls short of Lisp, add short TODO comments in Rust only where useful.

Do not modify `.missiond/v2/*.lisp` in this task.

## Non-goals

- Do not implement directive-compiler.
- Do not implement plan-compiler.
- Do not implement workflow-distiller.
- Do not implement methodology Lisp → YAML compiler.
- Do not add new MCP tools.
- Do not change SQL migrations unless absolutely necessary.
- Do not make `execute_mode="internal"` default.
- Do not remove bridge behavior.
- Do not stage or commit.

## Files likely touched

Expected:

- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`
- `crates/missiond-mcp/src/tools/knowledge/plan.rs`

Possibly:

- `crates/missiond-daemon/src/handlers/mod.rs` only if you need a reusable internal dispatch helper. Prefer not to churn it.

## Tests

Add focused unit tests in `plan.rs` where practical:

- bridge mode remains bridge and includes dispatch_strategy.
- internal mission_execution builds expected args / response shape.
- internal mission_flow_run without flow_id returns structured error.
- objective derivation from plan sexp is bounded.
- evidence sidecar append helper writes valid JSON if factored into testable helper.

## Acceptance commands

Run:

```bash
cargo build --workspace
cargo test -p missiond-daemon
cargo test -p missiond-mcp
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check
```

## Deliverable report

Report:

- Modified files.
- Action status:
  - `execute_mode=bridge`
  - `execute_mode=internal target=mission_execution`
  - `execute_mode=internal target=mission_task_delegate`
  - `execute_mode=internal target=mission_flow_run`
  - evidence sidecar
  - dispatch_strategy persistence/response status
- Tests run and results.
- Any dry-run/read-only/manual boundaries that remain.

Do not stage. Do not commit.
