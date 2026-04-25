# Lisp backfill note: parallel code tasks completed

请继续当前 Lisp backfill，不要重做，不要改 Rust/SQL/JS，不 stage，不 commit。

本文件只补充并行 code-alignment 的最新事实，避免 Lisp 状态落后。

## 已完成的新代码事实

### 1. plan-runner auto-selection v1 已完成

`plan-runner-auto-selection-v1-code-alignment` 已报告完成，修改范围：

- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`
- `crates/missiond-mcp/src/tools/knowledge/plan.rs`

需要在 Lisp 中把原先笼统的：

- “plan-runner 自动从 PLAN.lisp DAG 推断 target/dispatch_strategy/target_project 仍 pending”

调整为更精确：

- code-aligned partial: plan-runner auto-selection v1 已能从 PLAN sexp hints 保守解析 `:target` / `:target-tool` / `:tool` / `:flow-id` / `:dispatch-strategy` / `:parallelism` / `:target-project` / `:requested-cwd` / `:objective` / `:summary`。
- explicit args 仍优先。
- bridge / dry_run / internal success / dispatch_failed response 均带 `target_source` / `dispatch_strategy_source` / `plan_hint_summary`。
- `agent-team` strategy 会向 `mission_task_delegate` objective 注入字面提示 `使用 agent-team提高效率`，且幂等。

仍 pending 的准确表述：

- full PLAN DAG scheduler / 多节点 dependency execution 仍 pending。
- semantic interpretation of arbitrary PLAN.lisp beyond conservative key/value hints 仍 pending。
- auto QuestionEvent gates 仍 pending。
- file-first PLAN.lisp writer/sync 仍 pending。

不要继续写成“必须 caller 显式传 target/dispatch_strategy”，现在已经不准确；应写“caller 显式传参优先，缺省时 auto-selection v1 可从保守 hints 推断；无法安全推断时仍 structured MISSING_PARAM”。

### 2. generated flow loader / mission_flow_run discoverability 已完成

`generated-flow-loader-code-alignment` 已报告完成，修改范围：

- `crates/missiond-daemon/src/engine/flow/loader.rs`
- `crates/missiond-daemon/src/handlers/compute/flow_run.rs`
- `crates/missiond-mcp/src/tools/compute/flow_run.rs`

需要在 Lisp 中把原先的：

- “global generated flow registry / mission_flow_run 自动发现 .missiond/generated/flows pending”

调整为：

- code-aligned partial: `mission_flow_run` loader/search 已支持 project generated flows。
- search order:
  1. explicit `flow_path`
  2. `<project_root>/.missiond/generated/flows/<flow_id>.yaml`
  3. `$MISSIOND_HOME/flows/<flow_id>.yaml`
- `action=list` 可合并 generated + core flows，generated 优先去重。
- `action=run` 可用 `flow_id + project/target_project/cwd` 或 explicit `flow_path`。
- response 暴露 `flow_source` / `flow_path` / `searched_paths` / `project_root_status`。

仍 pending 的准确表述：

- richer project-root resolution via longest-prefix cwd resolver may remain TODO if code report says so。
- methodology semantic lifting / forge compiler 仍 pending。
- automatic record_execution/distill feedback link 仍 pending。

## 必须保持的架构边界

- Lisp 仍是架构 SSOT；不要按代码反向改变目标设计。
- `:implementation-targets` 是 current code-aligned entry points，不是最终分文件边界。
- file-first `.missiond/alignment/*.lisp`、`.missiond/plans/*.lisp`、`.missiond/workflows/*.lisp` 自动 writer/sync 仍 pending，除非代码实际写了这些文件。

## 验收

修完后运行：

```bash
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check .missiond/v2/intent-flow.lisp .missiond/v2/intent-intent-layer.lisp .missiond/v2/intent-tools.lisp .missiond/v2/intent-memory.lisp .missiond/v2/intent-worker.lisp
```
