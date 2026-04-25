# MissionD v2 code-alignment hardening: plan-runner v0 status failure semantics

只做代码同构后的硬化修补，不重新设计架构，不修改 `.missiond/v2/*.lisp`，不 stage，不 commit。

当前背景：

- 上一批已经实现 `mission_plan(action=execute)` 的 plan-runner v0。
- 默认 `execute_mode="bridge"` 继续返回 `next_call`。
- `execute_mode="internal"` 会调用 `mission_execution` / `mission_task_delegate` / `mission_flow_run`，成功后追加 evidence sidecar，并把 plan 标记为 `executing`。
- 本次只修一个语义问题：内部 tool 已成功，但后续 `plan_update_status(plan.id, PlanStatus::Executing)` 失败时，当前响应仍返回 `status: "executing"`。这会让调用方误以为 plan FSM 已经落库。

目标：

1. 在 `crates/missiond-daemon/src/handlers/knowledge/plan.rs` 中修正 `action_execute_internal` 的成功后置阶段：
   - inner handler 返回 non-error 后，仍然保持“inner side effect 已经成功”的事实。
   - evidence sidecar append 失败仍然不 abort，但必须在响应中显式暴露，例如：
     - `evidence_path: null`
     - `evidence_error: "..."`
     - `runner_status` 可保持 dispatched 或使用更精确的 partial 状态，按代码风格选择。
   - `plan_update_status` 失败时，不要返回 `status: "executing"`。
   - 建议返回：
     - `status: "dispatch_partial"`
     - `runner_status: "status_update_failed"`
     - `status_update_error: "..."`
     - 保留 `inner_result`、`target_tool`、`dispatch_strategy`、`plan_id`、`board_task_id`、`evidence_path` / `evidence_error`
   - `plan_update_status` 成功或 plan 原本就是 `Executing` 时，现有 `status: "executing"` / `runner_status: "dispatched"` 语义保持。

2. 不改变这些行为：
   - `execute_mode="bridge"` 默认路径不 dispatch，不改 plan 状态。
   - inner handler 返回 error 时，仍返回 `status: "dispatch_failed"`，不写 evidence，不改 plan 状态。
   - `dry_run=true` 仍不 dispatch、不写 evidence、不改 plan 状态。
   - `mission_flow_run` internal mode 仍要求显式 `flow_id`。
   - 不新增 MCP tool，不新增 migration。

3. 测试要求：
   - 优先加一个小的纯函数/响应构造 helper 测试，覆盖 `status_update_failed` 响应不会宣称 `executing`。
   - 如果现有结构不适合纯测，可以只加最小重构，不要引入重型 DB mock。
   - 保持已有 12 个 `handlers::knowledge::plan::tests` 通过。

验收命令：

```bash
cargo test -p missiond-daemon handlers::knowledge::plan::tests
cargo test -p missiond-daemon
cargo test -p missiond-mcp --lib
cargo build --workspace
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check
```

交付报告请列：

- 修改文件
- `execute_mode=bridge` 是否保持默认兼容
- inner-success + evidence failure 的响应状态
- inner-success + plan status update failure 的响应状态
- 测试结果
- 明确说明未修改 `.missiond/v2/*.lisp`、未 stage、未 commit
