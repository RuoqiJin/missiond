# Wave 13 / Task 00 — commit Wave12 task docs + obvious warning cleanup

你在 `/Users/jinchen/Projects/missiond` 项目根目录工作。

目标：把 Wave12 的任务书文件纳入版本库，并做一个极小 warning cleanup。不要启动新架构功能。

当前预期状态：
- 代码工作树应干净。
- 仅剩 `.missiond/claudecode/wave12-*.md` untracked。
- 已知 warning：`crates/missiond-daemon/src/handlers/knowledge/plan.rs` 里 `std::path::Path` unused。

允许修改 / stage：
- `.missiond/claudecode/wave12-*.md`
- `crates/missiond-daemon/src/handlers/knowledge/plan.rs` 仅限删除 unused `Path` import

禁止：
- 不要修改 `.missiond/v2/*.lisp`
- 不要碰 evidence_collector / plan_dag runtime 逻辑
- 不要 `git add .`
- 不要 stage 未列出的文件

验收命令：
- `cargo test -p missiond-daemon handlers::knowledge::plan::tests`
- `cargo build --workspace`
- `node scripts/check-architecture-lisp.mjs --all-v2`
- `git diff --check`

提交步骤：
1. `git status --short` 确认只有允许范围的 diff/untracked。
2. 只 stage 允许范围文件。
3. `git diff --cached --name-only` 必须是允许范围子集。
4. commit message 建议：
   `chore(wave12): archive task briefs and trim plan warning`

交付报告：
- commit hash
- 验收结果
- 是否仍有未跟踪/未提交文件

