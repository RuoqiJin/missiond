# MissionD v2 Parallel ClaudeCode Work Index

这些文件是给多个 ClaudeCode 工位并行执行的代码同构任务。共同规则：

- 只做代码向 Lisp 对齐，不重新设计架构。
- 不修改 `.missiond/v2/*.lisp`。
- 不 stage / commit `.missiond/v2/*.lisp`。
- 每个任务完成后都跑自己的 acceptance checks。
- 提交时只提交自己任务范围内的代码文件。

## Safe Parallel Lanes

1. Project-root spawn cwd
   - file: `/Users/jinchen/Projects/missiond/.missiond/claudecode/project-root-spawn-cwd-code-alignment.md`
   - likely touches: slot/PTY/compute spawn path
   - can run with: xjp-router, event-bus, incident

2. xjp-router embedding provider
   - file: `/Users/jinchen/Projects/missiond/.missiond/claudecode/xjp-router-embedding-code-alignment.md`
   - likely touches: llm/embedding path
   - can run with: project-root, event-bus, incident

3. Event-bus execution/capability usage extensions
   - file: `/Users/jinchen/Projects/missiond/.missiond/claudecode/event-bus-execution-capability-events-code-alignment.md`
   - likely touches: event types + two existing handlers
   - can run with: project-root, xjp-router

4. Incident remediation playbook
   - file: `/Users/jinchen/Projects/missiond/.missiond/claudecode/incident-remediation-playbook-code-alignment.md`
   - likely touches: incident/aiops/board remediation
   - can run with: xjp-router; usually safe with project-root

## Exclusive MCP Registration Lane

These tasks both add new MCP tools and may conflict in `tools/mod.rs` / `handlers/mod.rs`.
Run only one at a time unless you intentionally want to merge registration conflicts.

1. Directive / Plan / Workflow surfaces
   - file: `/Users/jinchen/Projects/missiond/.missiond/claudecode/directive-plan-workflow-surfaces-code-alignment.md`

2. Global instruction manager
   - file: `/Users/jinchen/Projects/missiond/.missiond/claudecode/global-instruction-manager-code-alignment.md`

## Suggested First Batch

Open 3-4 ClaudeCode sessions:

1. project-root spawn cwd
2. xjp-router embedding provider
3. event-bus execution/capability events
4. incident remediation playbook

Run directive/plan/workflow after those, or run it alone because it is broad and touches MCP registration.
