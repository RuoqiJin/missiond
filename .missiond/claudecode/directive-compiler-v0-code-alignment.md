# MissionD v2 code-alignment: directive-compiler actor v0

使用 agent-team 提高效率，但写入范围必须保持清晰。只做代码向 Lisp 对齐，不重新设计架构，不修改 `.missiond/v2/*.lisp`，不 stage，不 commit。

## 目标

把 `mission_directive(action=compile)` 从纯 dry-run/draft 管理面推进到 **directive-compiler actor v0**：

- 默认行为仍保持兼容：不传新参数时继续 dry-run，不调用 LLM。
- 显式 `compiler_mode="sonnet"` 时，调用 MissionD 现有 Sonnet chat 通道，把 user utterance 编译成可 review 的 directive sexp。
- `persist=true` 时，写入 directive 表 draft 行，等待人工 review/approve。
- 不新增 MCP tool，不新增 migration，不改 Lisp。

Lisp 锚点：

- `.missiond/v2/intent-flow.lisp :: F-intent-alignment-plan-execution-loop :: s2 intent-alignment-authoring / s3 alignment-review-gate`
- `.missiond/v2/intent-intent-layer.lisp :: section unified-entry-pipeline :: role alignment-author`
- `.missiond/v2/intent-memory.lisp :: module directive-layer :: file-first-artifacts :: intent-alignment-artifact`
- `.missiond/v2/intent-tools.lisp :: implemented-surface mission_directive`

## 写入范围

主要 ownership：

- `crates/missiond-daemon/src/handlers/knowledge/directive.rs`
- `crates/missiond-mcp/src/tools/knowledge/directive.rs`

允许只在必要时读：

- `crates/missiond-daemon/src/llm/sonnet_gateway.rs`
- `crates/missiond-daemon/src/llm/llm_gateway.rs`
- `crates/missiond-daemon/src/llm/minimax_client.rs` (`ChatMessage`)
- `crates/missiond-core/src/types/directive.rs`
- `crates/missiond-core/src/db/traits.rs`

不要改：

- `.missiond/v2/*.lisp`
- `plan.rs`
- `workflow.rs`
- DB migrations

## 期望行为

### 1. 参数扩展

给 `mission_directive` schema 和 handler 增加可选参数：

- `compiler_mode`: enum `["dry_run", "sonnet"]`, default `"dry_run"`
- `review_gate`: optional string/free-form, 记录人工 review gate 说明
- `affected_pillars`: optional array/string, 作为 prompt/context refs
- `non_goals`: optional array/string, 作为 prompt/context refs
- `acceptance`: optional array/string, 作为 prompt/context refs

保留原参数：

- `utterance`
- `source`
- `conversation_id`
- `persist`

### 2. dry_run 兼容

`compiler_mode` 缺省或 `"dry_run"`：

- 保持当前 dry-run 语义。
- 可以刷新 payload 字段名，但必须保留：
  - `status: "dry_run"`
  - `compiled_sexp_preview`
  - `persisted`
  - `directive_id` when `persist=true`
- 不调用 LLM。

### 3. sonnet 编译

`compiler_mode="sonnet"`：

- 若 `state.sonnet` 不可用，返回 structured error：
  - code 建议 `"LLM_UNAVAILABLE"` 或现有合适错误码
  - suggestion 明确说明可退回 `compiler_mode="dry_run"` 或启动 sonnet gateway。
- 调 `state.sonnet.as_ref().unwrap().call_interactive(...)`，或复用现有 `llm_gateway::call_sonnet_stateless`，按项目现有风格选一种。
- Prompt 必须要求模型只输出一个 Lisp sexp，不输出 Markdown 解释。
- 输出解析：
  - 支持去掉 ```lisp fenced code block。
  - trim 后必须非空。
  - 必须以 `(` 开头。
  - 必须括号平衡，且字符串里的括号不计数。
  - 顶层 head 建议允许 `directive` / `directive-draft` / `intent-alignment`；如果不匹配，返回 structured error，不落库。
- 成功响应：
  - `status: "compiled"`
  - `compiler_mode: "sonnet"`
  - `compiler_model: "claude-sonnet"` 或项目现有模型名
  - `compiled_sexp`
  - `persisted`
  - `directive_id` / `version` when persisted
  - `review_required: true`
  - `next_step: "review via mission_directive(action=approve) after human edit/review"`
- `persist=true` 时：
  - 写 directive 表，status 仍用 `DirectiveStatus::Draft`，不要自动 approve。
  - `references_json` 写入 source/conversation_id/compiler_mode/review_gate/affected_pillars/non_goals/acceptance。

## 测试要求

不要写需要真实 LLM 的测试。新增纯函数并测试：

- fenced code block 提取。
- 括号平衡检查能忽略字符串里的括号。
- 顶层 head 识别成功/失败。
- dry_run 默认不需要 sonnet。
- `compiler_mode` 非法时报 structured error。

运行验收：

```bash
cargo test -p missiond-daemon handlers::knowledge::directive::tests
cargo test -p missiond-daemon
cargo test -p missiond-mcp --lib
cargo build --workspace
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check
```

## 交付报告

请列：

- 修改文件
- action=compile 的 `dry_run` / `sonnet` / `persist=true` 状态
- LLM 不可用时的行为
- validation 规则
- 测试结果
- 明确说明未修改 `.missiond/v2/*.lisp`、未 stage、未 commit
