# Wave 14 / Task 04 — unified entry pipeline v1 consumes file-first + review gates

你在 `/Users/jinchen/Projects/missiond` 项目根目录工作。使用 agent-team提高效率。

前置：建议在 Task 01 和 Task 03 后执行。

目标：升级 Wave13 的 `unified_entry.rs` helper，让统一入口 v1 能使用 file-first writer 和 review gate policy，而不是只串 DB manager surfaces。

Ownership：
- `crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs`
- `crates/missiond-daemon/src/handlers/knowledge/directive.rs` 仅限暴露/复用 helper
- `crates/missiond-daemon/src/handlers/knowledge/plan.rs` 仅限暴露/复用 helper
- `crates/missiond-mcp/src/tools/knowledge/directive.rs` 或 `plan.rs` 仅限 schema 说明（如果现有 surface 承载）

禁止：
- 不要新增 MCP tool，除非先证明现有 surface 无法承载。
- 不要修改 `.missiond/v2/*.lisp`
- 不要修改 workflow/capability_usage/agent_execution
- 不要新增 DB migration
- 不要 `git add .`

功能要求：
1. v1 pipeline 参数支持：
   - `write_file`
   - `topic`
   - `overwrite_file`
   - `review_gate`
   - `project|target_project|cwd`
2. message-only path：
   - directive compile persist/write_file
   - 返回 file path + review question pointer
   - 不继续 plan compile，除非 caller 提供 approved directive id。
3. approved directive path：
   - plan compile persist/write_file
   - 返回 PLAN.lisp file path + review question pointer
4. approved plan path：
   - execute plan，支持 scheduler_mode/max_parallel_nodes forwarding
5. 每个 response 都带 `pipeline_stage`、`artifact_refs`、`next_step`。
6. 不自动 approve，不等待 review answer，不私自 spawn workstation。

测试要求：
- pipeline planner pure tests for file-first args forwarding。
- message -> directive file artifact response shape。
- approved directive -> plan file artifact response shape。
- approved plan -> execute with scheduler args forwarding。
- legacy no file-write path remains compatible。
- `cargo test -p missiond-daemon handlers::knowledge::unified_entry::tests`
- `cargo test -p missiond-daemon handlers::knowledge::directive::tests`
- `cargo test -p missiond-daemon handlers::knowledge::plan::tests`
- `cargo test -p missiond-daemon`
- `cargo test -p missiond-mcp --lib`
- `cargo build --workspace`
- `node scripts/check-architecture-lisp.mjs --all-v2`
- `git diff --check`

提交：
- scoped commit，只 stage ownership 文件。
- commit message:
  `feat(intent): route unified entry through file-first gates`

交付报告：
- commit hash
- v1 pipeline matrix
- whether any tool/schema changed
- remaining non-goals

