# Wave 13 — parallel dispatch index

先串行：

1. `/Users/jinchen/Projects/missiond/.missiond/claudecode/wave13-00-commit-wave12-docs-and-warning-cleanup.md`

然后建议顺序：

2. `/Users/jinchen/Projects/missiond/.missiond/claudecode/wave13-01-evidence-collector-integration.md`
   - 必须早做，因为 Task 02 会继续碰 `plan_dag.rs`。

3. `/Users/jinchen/Projects/missiond/.missiond/claudecode/wave13-02-plan-dag-runtime-v2.md`
   - 在 Task 01 后做，避免 `plan.rs/plan_dag.rs` 冲突。

可并行窗口：

4. `/Users/jinchen/Projects/missiond/.missiond/claudecode/wave13-03-unified-entry-flow-v0.md`
   - 可与 Task 02 并行，但不要改 `plan_dag.rs`。

5. `/Users/jinchen/Projects/missiond/.missiond/claudecode/wave13-04-lisp-backfill-wave13-status.md`
   - 等 Task 01/02/03 至少一项 commit 后，常驻 Lisp 工位回填。

最后做：

6. `/Users/jinchen/Projects/missiond/.missiond/claudecode/wave13-05-lisp-compression-l1-safe.md`
   - 仅在 Task 04 回填完成后执行。

通用执行协议：
- 所有代码任务从项目根目录 spawn。
- 代码任务默认新 ClaudeCode 会话。
- Lisp 任务使用常驻 Lisp 会话。
- 宽任务提示“使用 agent-team提高效率”。
- 每个任务 scoped commit：只 stage ownership 文件，报告 commit hash。
- 禁止 `git add .`。
- 禁止 stash/reset/checkout 其他任务 ownership 文件。

