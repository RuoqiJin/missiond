# Wave 14 — parallel dispatch index

先串行：

1. `/Users/jinchen/Projects/missiond/.missiond/claudecode/wave14-00-archive-wave13-task-docs.md`

然后可并行：

2. `/Users/jinchen/Projects/missiond/.missiond/claudecode/wave14-01-file-first-writer-integration.md`
   - 宽任务，建议单独新会话 + agent-team。
   - 会碰 directive/plan/workflow，其他任务避免同时改这些 writer 参数。

3. `/Users/jinchen/Projects/missiond/.missiond/claudecode/wave14-02-plan-node-events-and-live-evidence-refs.md`
   - 可与 01 并行，但如果出现 `plan.rs` 冲突，等 01 后再跑。

4. `/Users/jinchen/Projects/missiond/.missiond/claudecode/wave14-05-source-index-checker-implementation.md`
   - JS/Lisp checker 任务，可与代码任务并行。

5. `/Users/jinchen/Projects/missiond/.missiond/claudecode/wave14-06-event-domain-doc-cleanup.md`
   - 注释/doc 清理，可与 01/05 并行；避免和 02 同时改 event docs。

后续串行：

6. `/Users/jinchen/Projects/missiond/.missiond/claudecode/wave14-03-review-gate-autocreate-v1.md`
   - 建议在 01 后做，因为要消费 file-first artifact response。

7. `/Users/jinchen/Projects/missiond/.missiond/claudecode/wave14-04-unified-entry-pipeline-v1.md`
   - 建议在 01 + 03 后做。

8. `/Users/jinchen/Projects/missiond/.missiond/claudecode/wave14-07-lisp-backfill-and-l2-shard-plan.md`
   - 常驻 Lisp 工位，等前面至少一项 commit 后回填；最后做 L2 split plan。

通用执行协议：
- 所有代码任务从项目根目录 spawn。
- 代码任务默认新 ClaudeCode 会话。
- Lisp 任务使用常驻 Lisp 会话。
- 宽任务提示“使用 agent-team提高效率”。
- 每个任务 scoped commit：只 stage ownership 文件，报告 commit hash。
- 禁止 `git add .`。
- 禁止 stash/reset/checkout 其他任务 ownership 文件。

