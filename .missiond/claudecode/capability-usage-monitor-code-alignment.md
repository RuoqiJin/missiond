# MissionD v2 Code Alignment Task: Capability Usage Monitor

请按 MissionD v2 Lisp 架构做第二批代码同构：实现 capability usage monitor。

只做代码同构，不重新设计架构，不修改 Lisp。当前 `.missiond/v2` 的 Lisp 是工作树里的最新设计，请先读这些锚点：

- `/Users/jinchen/Projects/missiond/.missiond/v2/intent-memory.lisp` :: `system-support` :: `capability-usage-read-model`
- `/Users/jinchen/Projects/missiond/.missiond/v2/intent-flow.lisp` :: `F-capability-usage-monitoring`
- `/Users/jinchen/Projects/missiond/.missiond/v2/intent-intent-layer.lisp` :: `capability-evolution-governance`
- `/Users/jinchen/Projects/missiond/.missiond/v2/intent-tools.lisp` :: `future-surface mission_capability_usage`
- `/Users/jinchen/Projects/missiond/.missiond/v2/intent-event-bus.lisp` :: `planned-event-extensions` :: `CapabilityUsageObservability`

## Goals

1. 新增 `mission_capability_usage` MCP surface，actions: `snapshot` / `report` / `candidates` / `mark` / `ack`。
2. `snapshot` / `report` / `candidates` 必须从现有真实数据派生，不新增强 schema：
   - tool 调用来源优先用现有 `tool_calls` / `conversation_tool_calls` / audit projection。先调查真实表和 trait 名，不要假设。
   - flow 使用来源优先用 `event_log`、`board_tasks.flow_template` / `flow_context`、workflow / `mission_flow_run` 现有证据。
3. 输出 capability usage snapshot：`window`、`scope`、`generated_at`、`counts_by_capability`、`last_used_at`、`success/failure`、`source_coverage`、`candidates`、`protected_ids`。
4. candidates 分类至少覆盖：`active` / `quiet` / `stale` / `never-used` / `shadowed-by-better-capability` / `merge-candidate` / `protected`。
5. `mark` / `ack` 不允许删除 tool/flow，不允许改 registry。可优先用 `daemon_state` JSON 记录 review 状态；如果现有存储不适合，就实现 `dry_run` / `read-only` 并明确说明原因。
6. 不实现新的 event-bus enum/domain。本批只保留 TODO 或注释指向 planned `ObservabilityEvent::CapabilityUsageSnapshot` / `CapabilityStaleCandidate`，除非现有 bus 已天然支持且不破坏契约。
7. 保护项必须默认不进入删除候选：daemon bootstrap、memory/event-bus repair、`mission_execution`、`mission_intent`、`mission_forge_*`、manual recovery tools。
8. 所有 destructive 行为禁止。本工具只产生证据和治理候选。

## Acceptance

- `cargo build --workspace`
- `cargo test -p missiond-daemon`
- `cargo test -p missiond-mcp`
- `node scripts/check-architecture-lisp.mjs --all-v2`
- `git diff --check`

## Deliverables

- 列出修改文件。
- 列出每个 action 的实现状态：`full` / `dry-run` / `read-only`，并说明原因。
- 标明是否新增 migration；除非必要，不要新增。
- 不要 stage 或 commit `.missiond/v2` Lisp 文件。
