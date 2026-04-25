# MissionD v2 code-alignment: capability usage semantic evidence v1

使用 agent-team 提高效率，但写入范围必须保持清晰。只做代码向 Lisp 对齐，不重新设计架构，不修改 `.missiond/v2/*.lisp`，不 stage，不 commit。

## 目标

把 `mission_capability_usage` 从第一版 read-model 推进到 semantic evidence v1，补上上一批交付报告里明确留下的两个洞：

- `merge-candidate` bucket 不再永远为空；至少能基于 Lisp 里的显式 replacement / consolidation hints 产出候选。
- flow 使用证据不只来自 `board_tasks.flow_template`；尽量接入现有 event log / workflow execution stats 的只读证据源。
- 保持该工具的治理属性：只产出证据和候选，不删除 tool/flow，不改 registry。
- 不新增 MCP tool，不改 Lisp。仅在确实需要事件日志查询能力时允许小范围 DB trait/read-only helper。

Lisp 锚点：

- `.missiond/v2/intent-flow.lisp :: F-capability-usage-monitoring :: s3/s4/s5/s6`
- `.missiond/v2/intent-intent-layer.lisp :: capability-evolution-governance`
- `.missiond/v2/intent-tools.lisp :: future-surface mission_capability_usage`
- `.missiond/v2/intent-memory.lisp :: system-support :: capability-usage-read-model`

## 写入范围

主要 ownership：

- `crates/missiond-daemon/src/handlers/comm/capability_usage.rs`
- `crates/missiond-mcp/src/tools/comm/capability_usage.rs`

允许只在必要时改：

- `crates/missiond-core/src/db/traits.rs`
- `crates/missiond-core/src/db/pg/*.rs` read-only methods only

不要改：

- `.missiond/v2/*.lisp`
- `agent_execution.rs`
- `directive.rs`
- `plan.rs`
- `workflow.rs`
- DB migrations unless absolutely unavoidable; prefer existing tables/read paths.

## 期望行为

### 1. Semantic hint index

Add a small parser/indexer that reads current architecture Lisp text files as evidence, not as schema truth:

- `.missiond/v2/intent-tools.lisp`
- `.missiond/v2/intent-flow.lisp`
- `.missiond/v2/intent-intent-layer.lisp`

It should extract explicit, conservative hints only. Examples:

- `replacement`
- `preferred`
- `consolidated`
- `moved-to`
- `target-flow`
- `dispatch-status`
- `merge`
- `shadowed`

Do not infer from fuzzy name similarity alone. If no explicit target id can be found, leave candidate as stale/quiet and include note `semantic_hint_missing_target`.

### 2. merge-candidate bucket

Populate `merge-candidate` when all are true:

- current capability has low/zero usage according to existing counts,
- Lisp semantic hint names a better target capability or target flow,
- target capability exists in MCP registry / YAML flow registry / tool-backed-flow index,
- target is not the same id,
- target is not obviously less capable according to available evidence.

Each candidate must include:

- source id/kind
- replacement target id/kind
- evidence strings with file/anchor-ish context
- usage counts for source and target
- protected flags

Protected source or target must never become destructive by default; mark as review only.

### 3. Flow evidence expansion

Try to add one more read-only source for flow execution counts:

- existing workflow execution stats if present, or
- event log events that contain flow id/template, or
- existing board/task notes if that is the only available durable source.

Do not copy complex projection logic if it is risky. If no safe existing source exists, leave a clear `source_coverage` field explaining why.

Response `source_coverage` should distinguish:

- `conversation_tool_calls`
- `board_tasks.flow_template`
- `workflow_execution_stats` or `event_log_flow_events`
- `lisp_semantic_hints`
- `review_sidecar`

### 4. Governance safety

`mark` and `ack` behavior must remain non-destructive:

- Protected ids still reject destructive decisions.
- `merge` decision requires `replacement_target` or existing semantic hint target.
- `ack` still requires `follow_up_ref`.
- Sidecar format remains backward compatible.

## 测试要求

Add pure-function tests around parser and classifier:

- extracts explicit replacement/moved-to target from sample Lisp snippets.
- ignores fuzzy/ambiguous text with no target.
- classifies merge-candidate only when source is low usage and target exists.
- protected source/target downgrades destructive proposal to review/monitor.
- source_coverage includes semantic hints when parser runs.
- existing stale/never-used/protected tests still pass.

Run acceptance:

```bash
cargo test -p missiond-daemon handlers::comm::capability_usage::tests
cargo test -p missiond-daemon
cargo test -p missiond-mcp --lib
cargo test -p missiond-core --lib
cargo build --workspace
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check
```

## 交付报告

请列：

- 修改文件
- merge-candidate 的实现状态和限制
- 新增 flow evidence source 的实现状态；如果只能 read-only/dry-run，明确原因
- source_coverage 字段变化
- governance safety 是否保持
- 测试结果
- 明确说明未修改 `.missiond/v2/*.lisp`、未 stage、未 commit
