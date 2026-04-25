# Wave 12 — parallel dispatch index

先串行执行：

1. `/Users/jinchen/Projects/missiond/.missiond/claudecode/wave12-00-wave11-final-review-scoped-commit.md`

Wave 11 commit 完成后，下面可以并行派发，写入范围基本互不冲突：

2. `/Users/jinchen/Projects/missiond/.missiond/claudecode/wave12-01-mission-execution-scoped-commit-handoff.md`
   - owner: agent_execution only

3. `/Users/jinchen/Projects/missiond/.missiond/claudecode/wave12-02-plan-dag-scheduler-v1.md`
   - owner: plan / optional plan_dag

4. `/Users/jinchen/Projects/missiond/.missiond/claudecode/wave12-04-workflow-methodology-semantic-lifting-v0.md`
   - owner: workflow only

5. `/Users/jinchen/Projects/missiond/.missiond/claudecode/wave12-05-capability-usage-semantic-merge-v0.md`
   - owner: capability_usage only

6. `/Users/jinchen/Projects/missiond/.missiond/claudecode/wave12-06-lisp-source-index-expansion.md`
   - owner: source-index/architecture-dsl only; use resident Lisp session

不要和 Task 02 同时执行，除非明确协调 `plan.rs`：

7. `/Users/jinchen/Projects/missiond/.missiond/claudecode/wave12-03-evidence-collector-actor-v0.md`
   - touches plan.rs minimally; best run after Task 02 or assign same worker.

通用执行协议：
- 所有代码任务都必须从项目根目录 spawn。
- 代码任务默认使用新 ClaudeCode 会话；Lisp source-index 任务用常驻 Lisp 会话。
- 宽扫描/重构任务提示“使用 agent-team提高效率”。
- 每个任务必须 scoped commit：只 stage ownership 文件，报告 commit hash。
- 禁止 `git add .`。
- 禁止 stash/reset/checkout 其他任务 ownership 文件。

