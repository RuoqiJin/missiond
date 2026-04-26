# Wave 13 / Task 03 — unified entry pipeline v0 internal flow

你在 `/Users/jinchen/Projects/missiond` 项目根目录工作。使用 agent-team提高效率。

前置：建议在 Task 01 之后执行；可与 Task 02 并行，但避免改 `plan_dag.rs`。

目标：把现有管理面串成一个最小可执行统一入口：message → directive compile → review pointer → plan compile → review pointer → plan execute。v0 不做自动 approve，不绕过人工 review gate。

Ownership：
- 可新增 `crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs`
- `crates/missiond-daemon/src/handlers/knowledge/mod.rs`
- `crates/missiond-daemon/src/handlers/mod.rs` 仅限 dispatch wiring（如果新增 MCP surface）
- `crates/missiond-mcp/src/tools/knowledge/*` 仅限新增/接入 schema（若决定不新增 tool，则不改）
- 可最小调用 existing `directive.rs` / `plan.rs` / `workflow.rs` helper，但不要大改它们

架构约束：
- 优先不新增 MCP tool；如果能用 `mission_plan(action=execute)` / `mission_directive(action=compile)` 组合完成，就只做 internal helper。
- 如果确实需要新增 `mission_invoke`/`mission_message`，必须在报告里说明为何现有 83 tool 管理面不够，并同步 MCP count/test。
- 不自动 approve directive/plan。
- 不直接派 ClaudeCode 工位；只返回 next_step / next_call，或在明确 `execute_after_approval=true` 且 plan 已 approved 时调用 plan execute。

v0 行为：
1. 输入 message/source/context。
2. 调 directive compile（dry_run 或 sonnet 由 caller 参数指定），persist 可选。
3. 返回 directive review pointer。
4. 如果 caller 提供 approved directive id，则可继续 plan compile。
5. 返回 plan review pointer。
6. 如果 caller 提供 approved plan id + execute=true，则调用 mission_plan execute。
7. 每一步 response 都带 `pipeline_stage` 和 `next_step`。

测试要求：
- pure flow planner tests：message only、approved directive、approved plan execute、missing approval gate。
- handler tests 如新增 handler。
- `cargo test -p missiond-daemon handlers::knowledge`
- `cargo test -p missiond-daemon`
- `cargo test -p missiond-mcp --lib`
- `cargo build --workspace`
- `node scripts/check-architecture-lisp.mjs --all-v2`
- `git diff --check`

提交要求：
- scoped commit，只 stage ownership 文件。
- commit message 建议：
  `feat(intent): add unified entry pipeline helper`

交付报告：
- commit hash
- 是否新增 tool
- v0 pipeline 支持矩阵
- 明确未实现：auto approve / auto review answer / autonomous workstation dispatch

