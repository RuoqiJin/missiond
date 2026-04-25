# MissionD v2 code-alignment: plan-compiler actor v0

使用 agent-team 提高效率，但写入范围必须保持清晰。只做代码向 Lisp 对齐，不重新设计架构，不修改 `.missiond/v2/*.lisp`，不 stage，不 commit。

## 目标

把 `mission_plan(action=compile)` 从纯 dry-run/draft 管理面推进到 **plan-compiler actor v0**：

- 默认行为保持兼容：不传新参数时继续 dry-run，不调用 LLM。
- 显式 `compiler_mode="sonnet"` 时，从 approved directive / board task context 编译 PLAN.lisp-style sexp。
- `persist=true` 时写 plan 表 draft/awaiting_approval 行，等待人工 review/approve。
- 不影响已经完成的 `mission_plan(action=execute)` plan-runner v0。
- 不新增 MCP tool，不新增 migration，不改 Lisp。

Lisp 锚点：

- `.missiond/v2/intent-flow.lisp :: F-intent-alignment-plan-execution-loop :: s4 plan-authoring / s5 plan-review-gate`
- `.missiond/v2/intent-intent-layer.lisp :: section unified-entry-pipeline :: role plan-compiler`
- `.missiond/v2/intent-memory.lisp :: module directive-layer :: file-first-artifacts :: plan-artifact`
- `.missiond/v2/intent-tools.lisp :: implemented-surface mission_plan`

## 写入范围

主要 ownership：

- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`
- `crates/missiond-mcp/src/tools/knowledge/plan.rs`

允许只在必要时读：

- `crates/missiond-daemon/src/llm/sonnet_gateway.rs`
- `crates/missiond-daemon/src/llm/minimax_client.rs` (`ChatMessage`)
- `crates/missiond-core/src/types/directive.rs`
- `crates/missiond-core/src/db/traits.rs`

不要改：

- `.missiond/v2/*.lisp`
- `directive.rs`
- `workflow.rs`
- DB migrations

## 期望行为

### 1. 参数扩展

给 `mission_plan` schema 和 compile handler 增加可选参数：

- `compiler_mode`: enum `["dry_run", "sonnet"]`, default `"dry_run"`
- `directive_version`: optional integer; 缺省时取 directive version_chain head
- `target_project`: optional string/path context for prompt only
- `dispatch_strategy`: optional enum already used by execute; compile 可写入 plan sexp 的 node hints
- `parallelism`: optional enum/string, e.g. `"serial" | "agent-team" | "mixed"`
- `acceptance`: optional array/string
- `constraints`: optional array/string

保留原参数：

- `directive_id`
- `board_task_id`
- `persist`

### 2. dry_run 兼容

`compiler_mode` 缺省或 `"dry_run"`：

- 保持当前 dry-run 行为。
- 不调用 LLM。
- 保留 `status: "dry_run"` / `compiled_sexp_preview` / `sexp_hash_preview` / `persisted`。

### 3. sonnet 编译

`compiler_mode="sonnet"`：

- `board_task_id` 必须存在，因为 plan 表 FK 非空；即使 `persist=false` 也建议要求它以便 prompt 完整。
- `directive_id` 存在时：
  - 读取 directive（指定 `directive_version` 或 version_chain head）。
  - 默认要求 directive.status 是 `approved` 或 `compiled`。
  - 若状态不合格，返回 structured error，suggestion 指向 `mission_directive(action=approve)`。
  - 如确实需要调试，可支持 `allow_unapproved=true`，但响应必须标明。
- 读取 board task，缺失则 structured NOT_FOUND。
- 调 Sonnet 生成一个 plan sexp。
- Prompt 必须要求只输出一个 Lisp sexp，不输出解释。
- 输出验证：
  - 支持去掉 fenced code block。
  - 非空，以 `(` 开头。
  - 括号平衡，字符串里的括号不计数。
  - 顶层 head 建议允许 `plan` / `plan-draft` / `PLAN`。
  - 必须包含 `board_task_id` 或 board task id 字面值；否则返回 validation error，避免生成脱锚 plan。
- 成功响应：
  - `status: "compiled"`
  - `compiler_mode: "sonnet"`
  - `compiler_model: "claude-sonnet"` 或项目现有模型名
  - `compiled_sexp`
  - `sexp_hash`
  - `persisted`
  - `plan_id` / `version` when persisted
  - `review_required: true`
  - `next_step: "review then mission_plan(action=approve)"`
- `persist=true` 时：
  - 继续计算 next version per board task。
  - `status` 建议写 `PlanStatus::AwaitingApproval`，而不是直接 approved。
  - `compiler_model` 写模型名。
  - `compiled_from` 写 `directive/<id>:<version>` 或 `board_task/<id>`。

### 4. 不破坏 execute

- 不改 `action_execute` 默认 bridge 行为。
- 不改 internal dispatch 成功/partial 语义。
- 不改 evidence sidecar helper 的现有行为。

## 测试要求

不要写真实 LLM 测试。新增纯函数测试：

- fenced output extraction。
- paren balance ignores strings。
- top head validation。
- board_task id anchoring validation。
- dry_run 默认兼容。
- invalid `compiler_mode` structured error。

保留并通过现有 plan tests，尤其是 plan-runner v0 的 16 个测试。

运行验收：

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
- compile dry_run / sonnet / persist 状态
- directive approval gate 行为
- execute 行为是否保持兼容
- 测试结果
- 明确说明未修改 `.missiond/v2/*.lisp`、未 stage、未 commit
