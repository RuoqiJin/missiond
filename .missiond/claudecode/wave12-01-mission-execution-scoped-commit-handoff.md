# Wave 12 / Task 01 — mission_execution scoped commit handoff code-alignment

你在 `/Users/jinchen/Projects/missiond` 项目根目录工作。使用 agent-team提高效率。

前置：必须在 Wave 11 scoped commit 完成后执行。

目标：把 Lisp 蓝图里的双平面交付协议落到 `mission_execution`：
- execution companion Lisp = control plane
- scoped git commit = durability plane

Ownership：
- `crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs`
- `crates/missiond-mcp/src/tools/knowledge/agent_execution.rs`
- 如测试需要，可最小修改同文件内 tests。

不要修改：
- `.missiond/v2/*.lisp`
- plan/workflow/directive/capability_usage
- DB migrations

功能要求：
1. `mission_execution(action=complete)` 接收可选字段：
   - `changed_files: [string]`
   - `staged_files: [string]`
   - `commit_hash: string`
   - `commit_status: enum not-required|pending|committed|blocked|skipped`
   - `commit_blocker: string?`
2. companion log 的 `completions` block 写入这些字段。
3. `status` 和 `list` 能暴露 completion durability 信息。
4. `audit` 新增 read-only 检查：
   - code-delivery completion 若 `commit_status=committed` 但无 `commit_hash` -> finding
   - `commit_status=blocked` 但无 `commit_blocker` -> finding
   - `staged_files` 不属于 claim scope 时 -> scoped-commit-violation finding
5. 不要让 daemon 自动执行 git commit；本任务只接 schema、持久化、status/audit。
6. legacy execution 文件缺字段必须继续 parse。

测试要求：
- 新增/更新 agent_execution unit tests，覆盖 legacy parse、complete 写字段、audit finding。
- `cargo test -p missiond-daemon handlers::knowledge::agent_execution::tests`
- `cargo test -p missiond-daemon`
- `cargo test -p missiond-mcp --lib`
- `cargo build --workspace`
- `node scripts/check-architecture-lisp.mjs --all-v2`
- `git diff --check`

交付：
- scoped commit，只 stage 本任务 ownership 文件。
- commit message 建议：
  `feat(execution): record scoped commit handoff metadata`
- 报告 commit hash、修改文件、测试结果、未做边界。

