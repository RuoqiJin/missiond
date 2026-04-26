# Wave 14 / Task 01 — file-first writer integration for directive / plan / workflow

你在 `/Users/jinchen/Projects/missiond` 项目根目录工作。使用 agent-team提高效率。

前置：先完成 Wave14 Task 00。

目标：把 Wave11 的 `file_artifacts.rs` foundation 真正接入 directive / plan / workflow writer 主路径。当前 Lisp 已声明 file-first SSOT，但代码仍主要写 DB row / sidecar，`file_artifacts` 仍是 foundation/dead_code。此任务要让 `write_file=true` 走统一 helper。

Ownership：
- `crates/missiond-daemon/src/handlers/knowledge/file_artifacts.rs`
- `crates/missiond-daemon/src/handlers/knowledge/directive.rs`
- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`
- `crates/missiond-daemon/src/handlers/knowledge/workflow.rs`
- `crates/missiond-mcp/src/tools/knowledge/directive.rs`
- `crates/missiond-mcp/src/tools/knowledge/plan.rs`
- `crates/missiond-mcp/src/tools/knowledge/workflow.rs`

禁止：
- 不要修改 `.missiond/v2/*.lisp`
- 不要修改 `plan_dag.rs` / `evidence_collector.rs` / `agent_execution.rs` / `capability_usage.rs`
- 不要新增 DB migration
- 不要 `git add .`

功能要求：
1. `mission_directive(action=compile, persist=true, write_file=true, topic=...)`
   - 写 DB draft 后，写 `<project_root>/.missiond/alignment/<topic>/intent-alignment.lisp`
   - 使用 `ArtifactKind::IntentAlignment` + `atomic_write_artifact`
   - `overwrite_file=false` 默认拒覆
2. `mission_plan(action=compile, persist=true, write_file=true, topic=...)`
   - 写 DB awaiting_approval 后，写 `<project_root>/.missiond/plans/<topic>/PLAN.lisp`
   - 使用 `ArtifactKind::Plan`
3. `mission_workflow(action=distill|compile_methodology, persist=true, write_file=true, topic|name=...)`
   - 写 `<project_root>/.missiond/workflows/<topic>.lisp`
   - 使用 `ArtifactKind::Workflow`
4. 项目根解析统一走 `slot_orchestrator::project_root::resolve_target_project_root`。
   - `project` 注册 id 优先
   - `cwd` 必须绝对路径
   - `target_project` fallback
   - 禁止 process cwd fallback
5. partial 语义：
   - DB 已写但 file 写失败，response 必须 `status="partial"` 或带 `file_write_error`，并返回 id/version。
   - 不回滚 DB row，不吞错误。
6. response 增加：
   - `file_written`
   - `file_path`
   - `file_sha256`
   - `file_bytes`
   - `file_created`
   - `file_overwritten`
7. 减少或消除 `file_artifacts.rs` dead_code warning；若仍有 foundation-only API，必须加 `#[allow(dead_code)]` 和原因。

测试要求：
- directive compile writer tests：missing topic、no overwrite、success path、partial path helper。
- plan compile writer tests 同上。
- workflow distill/compile_methodology writer tests 至少覆盖一种 persist path。
- `cargo test -p missiond-daemon handlers::knowledge::file_artifacts::tests`
- `cargo test -p missiond-daemon handlers::knowledge::directive::tests`
- `cargo test -p missiond-daemon handlers::knowledge::plan::tests`
- `cargo test -p missiond-daemon handlers::knowledge::workflow::tests`
- `cargo test -p missiond-daemon`
- `cargo test -p missiond-mcp --lib`
- `cargo build --workspace`
- `node scripts/check-architecture-lisp.mjs --all-v2`
- `git diff --check`

提交：
- scoped commit，只 stage ownership 文件。
- commit message:
  `feat(knowledge): write file-first artifacts from compiler actors`

交付报告：
- commit hash
- 三类 artifact 的路径和行为矩阵
- partial/error 语义
- 是否仍有 file_artifacts dead_code warning

