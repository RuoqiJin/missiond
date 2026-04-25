# Wave 12 / Task 03 — evidence collector actor v0

你在 `/Users/jinchen/Projects/missiond` 项目根目录工作。使用 agent-team提高效率。

前置：必须在 Wave 11 scoped commit 完成后执行。若 Task 02 同时运行，请不要改 `plan.rs` 的 scheduler 逻辑；本任务只做 evidence collector helper/surface。

目标：把现在分散的 plan-runner evidence sidecar 写入升级成一个可复用 evidence collector v0。

Ownership：
- 可新增 `crates/missiond-daemon/src/handlers/knowledge/evidence_collector.rs`
- `crates/missiond-daemon/src/handlers/knowledge/mod.rs`
- `crates/missiond-daemon/src/handlers/knowledge/plan.rs` 仅允许最小接入 helper
- `crates/missiond-mcp/src/tools/knowledge/plan.rs` 仅允许 schema 描述补充

不要修改：
- `.missiond/v2/*.lisp`
- workflow/directive/capability_usage/agent_execution
- DB migrations

功能要求：
1. 提供 helper：append evidence entry with source/kind/timestamp/schema_version。
2. 支持收集：
   - inner dispatch result summary
   - verification command list/result summary
   - git diff stat / changed files
   - commit_hash / commit_status（若 caller 提供）
   - ExecutionEvent references（如果当前可用则引用 event id/source；不可用则标 unavailable）
3. `mission_plan(action=record_evidence)` 增加 `evidence_kind` 与 `source` 参数，但保持旧调用兼容。
4. evidence JSON 继续是 file-first sidecar，不新增 DB migration。
5. sidecar 写失败必须显式返回 structured error 或 partial status，不能吞掉。

测试要求：
- pure tests：entry normalization、legacy evidence wrapping、commit metadata preservation。
- sidecar append test：多 entry 保序，schema_version 存在。
- `cargo test -p missiond-daemon handlers::knowledge::evidence_collector`
- `cargo test -p missiond-daemon handlers::knowledge::plan::tests`
- `cargo test -p missiond-daemon`
- `cargo test -p missiond-mcp --lib`
- `cargo build --workspace`
- `node scripts/check-architecture-lisp.mjs --all-v2`
- `git diff --check`

交付：
- scoped commit，只 stage 本任务 ownership 文件。
- commit message 建议：
  `feat(plan): add evidence collector helper`
- 报告 commit hash、evidence entry schema、测试结果。

