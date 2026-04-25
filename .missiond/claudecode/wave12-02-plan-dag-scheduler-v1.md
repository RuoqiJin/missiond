# Wave 12 / Task 02 — PLAN DAG scheduler v1 minimal code-alignment

你在 `/Users/jinchen/Projects/missiond` 项目根目录工作。使用 agent-team提高效率。

前置：必须在 Wave 11 scoped commit 完成后执行。

目标：在现有 `mission_plan(action=execute, execute_mode=internal)` 单节点 runner 之上，增加一个最小可执行的 PLAN DAG scheduler v1。只支持显式 node schema，不做任意 Lisp 语义解释。

Ownership：
- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`
- 可新增 `crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs`
- `crates/missiond-daemon/src/handlers/knowledge/mod.rs`
- `crates/missiond-mcp/src/tools/knowledge/plan.rs`

不要修改：
- workflow/directive/capability_usage/agent_execution
- DB migrations
- `.missiond/v2/*.lisp`

输入契约：
- 新增可选参数：`scheduler_mode`
  - default/current: 保持 v0 单节点行为
  - `dag_v1`: 启用 DAG scheduler
- DAG v1 只解析 PLAN.lisp 中明确节点 form，例如：
  `(node :id "n1" :target "mission_task_delegate" :objective "..." :depends-on ["n0"] :dispatch-strategy "agent-team" :target-project "missiond")`
- 支持字段：`id / target / objective / depends-on / condition / failure-policy / timeout-ms / dispatch-strategy / target-project / requested-cwd / flow-id`
- 不支持的字段保留到 `node_hint_summary`，不得 silently 丢弃。

执行要求：
1. 构建 DAG 并校验：
   - node id 唯一
   - depends-on 指向存在
   - 无环
   - target 必须是 mission_execution / mission_task_delegate / mission_flow_run 之一
2. v1 首批可以顺序执行 ready nodes；并发执行可标 pending，不强求。
3. 每个 node 写 evidence sidecar：node_id、state transition、target、dispatch_strategy、inner_result/error。
4. failure-policy 首批支持：
   - `fail-fast` 默认
   - `continue` 跳过失败节点的下游并继续独立节点
5. dry_run=true 返回 DAG plan，不执行 inner dispatch、不写 evidence。
6. 复用现有 build_internal_dispatch_args / internal dispatch 路径，避免复制三套 handler 调用。

测试要求：
- DAG parser pure tests：唯一 id、depends missing、cycle、valid chain、unsupported metadata preservation。
- dry_run response test。
- sequential execution helper pure tests。
- `cargo test -p missiond-daemon handlers::knowledge::plan::tests`
- `cargo test -p missiond-daemon`
- `cargo test -p missiond-mcp --lib`
- `cargo build --workspace`
- `node scripts/check-architecture-lisp.mjs --all-v2`
- `git diff --check`

交付：
- scoped commit，只 stage 本任务 ownership 文件。
- commit message 建议：
  `feat(plan): add minimal DAG scheduler mode`
- 报告 commit hash、支持/不支持的 DAG 语义、测试结果。

