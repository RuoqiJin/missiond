# MissionD v2 Code Alignment Task: Incident Remediation Playbook

请按 MissionD v2 Lisp 架构做代码同构：补实 `F-incident-reaction` 的 remediation playbook，让 `mission_incident` 不只是 `test/list`，而能围绕 incidents 生成/跟踪修复任务。

只做代码同构，不重新设计架构，不修改 `.missiond/v2/*.lisp`。当前 Lisp 是工作树里的最新设计，请先读这些锚点：

- `/Users/jinchen/Projects/missiond/.missiond/v2/intent-flow.lisp` :: `F-incident-reaction`
- `/Users/jinchen/Projects/missiond/.missiond/v2/intent-tools.lisp` :: `mission_incident`
- `/Users/jinchen/Projects/missiond/.missiond/v2/intent-memory.lisp` :: `system-support` :: `incidents`
- `/Users/jinchen/Projects/missiond/.missiond/v2/intent-system-layer.lisp` :: `infra-aiops-tick`

## Parallel Scope

This task owns the incident/remediation lane:

- incident handler code
- aiops incident processing
- board task creation/linking for remediation
- existing `mission_incident` schema if new actions are needed

Avoid adding unrelated MCP tools. If you must edit shared MCP registration files, keep it minimal.

## Goals

1. Inspect current `mission_incident` implementation and `aiops::process_incident`.
2. Extend `mission_incident` actions if not already present:
   - `test`
   - `list`
   - `get`
   - `remediate`
   - `status`
   - `close`
3. Implement remediation flow:
   - normalize incident source/severity/title/evidence
   - persist/dedupe incident row
   - classify remediation target
   - create or link a board task for remediation
   - optionally dispatch to `mission_task_delegate` only when safe and explicit
   - add progress notes
4. Auto-close/recovery:
   - if health recovers and a linked remediation board task exists, close or annotate according to existing aiops conventions
   - do not close user-owned tasks blindly
5. Keep evidence-first output:
   - incident id
   - board task id if created/linked
   - remediation status
   - next action

## Non-Goals

- Do not redesign the incidents schema unless unavoidable.
- Do not modify `.missiond/v2/*.lisp`.
- Do not auto-run risky shell commands.
- Do not dispatch agents by default unless the existing system already does and the action explicitly requests it.

## Acceptance

- `cargo build --workspace`
- `cargo test -p missiond-core --lib`
- `cargo test -p missiond-daemon`
- `cargo test -p missiond-mcp`
- `node scripts/check-architecture-lisp.mjs --all-v2`
- `git diff --check`

Add focused tests where practical:

- duplicate incident dedupe
- `remediate` creates or links a board task
- `status` reads incident + remediation task
- `close` refuses unsafe close without explicit reason/actor

## Deliverables

- List modified files.
- For each action, mark `full` / `partial` / `read-only`.
- State whether agent dispatch is implemented or intentionally manual.
- State any schema/migration change.
