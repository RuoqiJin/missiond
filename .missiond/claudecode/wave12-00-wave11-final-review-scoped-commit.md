# Wave 12 / Task 00 — Wave 11 final review + scoped commit

你在 `/Users/jinchen/Projects/missiond` 项目根目录工作。

目标：先把当前 Wave 11 工作树做最终验收并 scoped commit，避免后续并行任务再次被 stash/pop/Edit 回退。不要做新功能。

必须遵守：
- 只处理下列 Wave 11 文件，不要 `git add .`。
- 不要改语义，除非验收失败且需要最小修复。
- 如果发现文件清单外还有工作树修改，先报告，不要 stage。
- commit 后在报告里给出 commit hash。

允许 stage 的文件：
- `.missiond/v2/architecture-dsl.lisp`
- `.missiond/v2/intent-flow.lisp`
- `.missiond/v2/intent-intent-layer.lisp`
- `.missiond/v2/intent-memory.lisp`
- `.missiond/v2/intent-pillar-source-index.lisp`
- `.missiond/v2/intent-tools.lisp`
- `.missiond/v2/intent-worker.lisp`
- `.missiond/v2/intent.lisp`
- `crates/missiond-core/src/event/events/execution.rs`
- `crates/missiond-daemon/src/handlers/comm/capability_usage.rs`
- `crates/missiond-daemon/src/handlers/compute/flow_run.rs`
- `crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs`
- `crates/missiond-daemon/src/handlers/knowledge/mod.rs`
- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`
- `crates/missiond-daemon/src/handlers/knowledge/workflow.rs`
- `crates/missiond-daemon/src/handlers/knowledge/file_artifacts.rs`
- `crates/missiond-daemon/src/handlers/knowledge/review_gate.rs`
- `crates/missiond-mcp/src/tools/comm/capability_usage.rs`
- `.missiond/claudecode/wave11-*.md`

验收命令：
- `cargo test -p missiond-core --lib`
- `cargo test -p missiond-daemon`
- `cargo test -p missiond-mcp --lib`
- `cargo build --workspace`
- `node scripts/check-architecture-lisp.mjs --all-v2`
- `git diff --check`

提交步骤：
1. 跑验收。
2. `git add` 仅添加上面的文件。
3. `git diff --cached --name-only`，确认 staged 文件是上面清单的子集。
4. commit message 建议：
   `feat(wave11): file-first execution governance and correction pass`
5. 报告：commit hash、验收结果、是否有未 stage 的非 Wave11 文件。

