# MissionD wave 11 Lisp design: PLAN DAG scheduler architecture

使用常驻 Lisp 架构会话执行。可以使用 agent-team 提高效率，但最终由一个主 agent 统一落笔。只改 `.missiond/v2/*.lisp`，不改 Rust/SQL/JS，不 stage，不 commit。

## 目标

先把完整 PLAN DAG scheduler 的 Lisp 架构补清楚，不做代码实现。

当前代码已有：

- `mission_plan(action=execute, execute_mode=internal)`
- target: `mission_execution` / `mission_task_delegate` / `mission_flow_run`
- auto-selection v1: 保守解析 `:target / :flow-id / :dispatch-strategy / :parallelism / :target-project / :requested-cwd`
- evidence sidecar 自动追加 `plan_runner_dispatch`

缺的是完整 DAG scheduler：

- 多节点 dependency。
- 并发 dispatch。
- per-node retry/failure policy。
- condition/gate。
- rollback/compensation。
- node evidence aggregation。

## 写入范围

允许修改：

- `.missiond/v2/intent-flow.lisp`
- `.missiond/v2/intent-intent-layer.lisp`
- `.missiond/v2/intent-tools.lisp`
- `.missiond/v2/intent-memory.lisp`
- `.missiond/v2/intent-worker.lisp` only if worker dispatch substrate needs cross-ref
- `.missiond/v2/intent.lisp` status summary if needed

不要修改：

- Rust / SQL / JS
- `.missiond/intent-mcp-defs.lisp`

## 设计要求

按 ingress → logic-core → egress 写清楚：

### Ingress

- approved PLAN.lisp or plan row.
- target_project / requested_cwd / dispatch_strategy / parallelism hints.
- existing evidence sidecar.

### Logic core

按阶段写：

1. load-plan-graph
2. validate-node-schema
3. resolve-target-project-root
4. build-ready-set
5. acquire execution claim / lease
6. dispatch ready nodes
7. collect node evidence
8. update node state
9. handle retry/failure/rollback
10. mark plan succeeded/failed/partial
11. trigger record_execution/distill candidate

### Egress

- plan FSM update。
- evidence sidecar node entries。
- mission_execution companion log。
- future events。
- workflow distill trigger candidate。

## 状态标注

必须明确：

- 当前 code-aligned: plan-runner v0 + auto-selection v1。
- architecture-designed pending: full DAG scheduler。
- 不要把 full DAG 写成已实现。

## 验收

- `node scripts/check-architecture-lisp.mjs --all-v2`
- `git diff --check .missiond/v2/intent-flow.lisp .missiond/v2/intent-intent-layer.lisp .missiond/v2/intent-tools.lisp .missiond/v2/intent-memory.lisp .missiond/v2/intent-worker.lisp .missiond/v2/intent.lisp`

## 交付报告

说明：

- 改了哪些 Lisp section。
- full DAG scheduler 的最小 node schema。
- 还需后续代码同构的 target files。

