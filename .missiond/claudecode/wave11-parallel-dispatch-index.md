# MissionD wave 11 parallel dispatch index

把下面任务分别发给不同 ClaudeCode 会话。除非任务文件另有说明，统一规则：

- 使用 agent-team 提高效率。
- 只做代码向 Lisp 对齐，不重新设计架构。
- 不 stage，不 commit。
- 每个会话只改自己 ownership 范围内的文件。
- 如果发现 Lisp 与代码事实不一致，先报告，不要擅自改 `.missiond/v2/*.lisp`，除非该任务明确是 Lisp-only。
- 验收必须跑任务文件列出的命令，并在报告里说明 full / partial / dry-run / read-only 状态。

## A 组：现在可并行

这些互不重叠，可以同时开新 ClaudeCode 会话：

1. `.missiond/claudecode/wave11-execution-event-dispatch-metadata-code-alignment.md`
2. `.missiond/claudecode/wave11-flow-run-longest-prefix-project-root-code-alignment.md`
3. `.missiond/claudecode/wave11-capability-usage-workflow-stats-code-alignment.md`
4. `.missiond/claudecode/wave11-lisp-source-index-precompression-design.md`
5. `.missiond/claudecode/wave11-file-artifact-foundation-code-alignment.md`

## B 组：等 foundation 完成后可并行

先完成 `wave11-file-artifact-foundation-code-alignment.md`，再开这三个：

6. `.missiond/claudecode/wave11-directive-file-first-writer-code-alignment.md`
7. `.missiond/claudecode/wave11-plan-file-first-writer-code-alignment.md`
8. `.missiond/claudecode/wave11-workflow-file-first-writer-code-alignment.md`

## C 组：等 directive/plan writer 稳定后执行

9. `.missiond/claudecode/wave11-review-gate-question-event-code-alignment.md`

## D 组：设计先行，可在 Lisp 常驻会话做

10. `.missiond/claudecode/wave11-plan-dag-scheduler-lisp-design.md`

