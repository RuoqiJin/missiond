# MissionD v2 code-alignment: workflow-distiller actor v0

使用 agent-team 提高效率，但写入范围必须保持清晰。只做代码向 Lisp 对齐，不重新设计架构，不修改 `.missiond/v2/*.lisp`，不 stage，不 commit。

## 目标

把 `mission_workflow(action=distill)` 从纯 dry-run/draft 管理面推进到 **workflow-distiller actor v0**：

- 默认行为保持兼容：不传新参数时继续 dry-run，不调用 LLM。
- 显式 `distill_mode="sonnet"` 时，从 succeeded plan + evidence sidecar 生成可复用 workflow sexp 和 match_rules。
- `persist=true` 时写 workflow 表 draft/template 行。
- 不实现 methodology Lisp → YAML compiler；那是下一波，避免同文件大改。
- 不新增 MCP tool，不新增 migration，不改 Lisp。

Lisp 锚点：

- `.missiond/v2/intent-flow.lisp :: F-intent-alignment-plan-execution-loop :: s8 workflow-distillation`
- `.missiond/v2/intent-intent-layer.lisp :: section unified-entry-pipeline :: role workflow-distiller`
- `.missiond/v2/intent-memory.lisp :: module directive-layer :: file-first-artifacts :: workflow-artifact`
- `.missiond/v2/intent-tools.lisp :: implemented-surface mission_workflow`

## 写入范围

主要 ownership：

- `crates/missiond-daemon/src/handlers/knowledge/workflow.rs`
- `crates/missiond-mcp/src/tools/knowledge/workflow.rs`

允许只在必要时读：

- `crates/missiond-daemon/src/llm/sonnet_gateway.rs`
- `crates/missiond-daemon/src/llm/minimax_client.rs` (`ChatMessage`)
- `crates/missiond-core/src/types/directive.rs`
- `crates/missiond-core/src/db/traits.rs`
- `crates/missiond-daemon/src/handlers/knowledge/plan.rs` only to understand sidecar convention; do not edit it.

不要改：

- `.missiond/v2/*.lisp`
- `directive.rs`
- `plan.rs`
- DB migrations
- `compile_methodology` / `run_methodology` beyond schema docs if absolutely necessary

## 期望行为

### 1. 参数扩展

给 `mission_workflow` schema 和 distill handler 增加可选参数：

- `distill_mode`: enum `["dry_run", "sonnet"]`, default `"dry_run"`
- `project`: optional project id for resolving `<project>/.missiond/v2/plans/<plan_id>.evidence.json`
- `name`: existing; persist=true 仍必填
- `match_hint`: optional array/string
- `protected`: optional boolean, only recorded in match_rules if provided
- `min_evidence`: optional integer, default 1 for sonnet mode unless `allow_missing_evidence=true`
- `allow_missing_evidence`: optional boolean, default false

### 2. dry_run 兼容

`distill_mode` 缺省或 `"dry_run"`：

- 保持当前 dry-run 行为。
- 不调用 LLM。
- 保留 `status: "dry_run"` / `compiled_sexp_preview` / `persisted`。

### 3. sonnet distill

`distill_mode="sonnet"`：

- 读取 plan，要求 `PlanStatus::Succeeded`；否则保持当前 structured error。
- 尝试读取 evidence sidecar：
  - `<project_root>/.missiond/v2/plans/<plan_id>.evidence.json`
  - project root 解析复用当前 `resolve_project_root`。
  - 文件不存在且 `allow_missing_evidence=false` 时返回 structured error。
  - 文件存在但 JSON parse 失败时返回 structured error，不落库。
- 调 Sonnet 生成结构化结果。建议 prompt 要求只输出 JSON，形如：
  ```json
  {
    "workflow_sexp": "(workflow ...)",
    "match_rules": {"tokens": [], "intents": [], "tools": [], "flows": []},
    "summary": "...",
    "reusability_score": 0.0
  }
  ```
- 输出验证：
  - 支持去掉 fenced code block。
  - JSON parse 必须成功。
  - `workflow_sexp` 非空、以 `(` 开头、括号平衡。
  - `match_rules` 必须是 object。
  - 如果 `name` 提供，`workflow_sexp` 或 match_rules 中应包含 name；若不包含，允许但响应里给 `warnings`。
- 成功响应：
  - `status: "distilled"`
  - `distill_mode: "sonnet"`
  - `compiler_model: "claude-sonnet"` 或项目现有模型名
  - `workflow_sexp`
  - `match_rules`
  - `evidence_path`
  - `persisted`
  - `workflow_id` when persisted
  - `review_required: true`
- `persist=true` 时：
  - `name` 必填。
  - 调 `workflow_insert(name, workflow_sexp, match_rules, Some(plan_id))`。
  - 不自动执行 workflow。

## 测试要求

不要写真实 LLM 测试。新增纯函数测试：

- fenced JSON extraction。
- distiller JSON parse 成功/失败。
- workflow_sexp paren balance ignores strings。
- match_rules 必须 object。
- missing evidence gate 的小 helper。
- dry_run 默认兼容。

运行验收：

```bash
cargo test -p missiond-daemon handlers::knowledge::workflow::tests
cargo test -p missiond-daemon
cargo test -p missiond-mcp --lib
cargo build --workspace
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check
```

## 交付报告

请列：

- 修改文件
- distill dry_run / sonnet / persist 状态
- evidence sidecar 读取与缺失策略
- compile_methodology/run_methodology 是否未改
- 测试结果
- 明确说明未修改 `.missiond/v2/*.lisp`、未 stage、未 commit
