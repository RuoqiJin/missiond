# MissionD wave 11 code-alignment: capability usage workflow stats

使用 agent-team 提高效率，但写入范围必须保持清晰。只做代码向 Lisp 对齐，不重新设计架构，不修改 `.missiond/v2/*.lisp`，不 stage，不 commit。

## 目标

`mission_capability_usage` semantic evidence v1 已有 5 sources，但 workflow execution stats / success rate 仍 pending。本任务把 workflow 表里的执行统计接入 read-model，作为第 6 个 evidence lane 或增强 existing flow lane。

## Lisp 锚点

- `.missiond/v2/intent-flow.lisp :: F-capability-usage-monitoring :: pending workflow stats / success rate`
- `.missiond/v2/intent-memory.lisp :: capability-usage-read-model`
- `.missiond/v2/intent-tools.lisp :: mission_capability_usage`

## 写入范围

允许修改：

- `crates/missiond-daemon/src/handlers/comm/capability_usage.rs`
- `crates/missiond-mcp/src/tools/comm/capability_usage.rs` only if response/schema docs need field description

如果 store trait 缺必要方法，允许最小修改：

- `crates/missiond-core/src/db/traits.rs`
- `crates/missiond-core/src/db/pg/directive.rs`

不要修改：

- workflow.rs
- plan.rs
- DB migrations unless absolutely unavoidable
- `.missiond/v2/*.lisp`

## 行为要求

Read workflow stats from existing workflow table fields, likely:

- executions
- success_count
- failure_count
- avg_cost_usd
- last_used_at
- match_rules / name

Integrate into `mission_capability_usage(action=snapshot|report|candidates)`:

- `source_coverage.sources.workflow_execution_stats = {status,count,note}`
- Rows for workflows/flows should include success/failure evidence when mapped.
- Classification can use workflow last_used_at/success_count as evidence, but do not auto-remove or auto-merge.
- If DB query fails, mark lane unavailable and keep main response read-only.

Mapping rule:

- Do not invent fuzzy semantic mapping. Use explicit workflow name/id and match_rules references only.
- If no reliable flow id mapping exists, expose stats in source coverage / evidence but do not force classification.

## Tests

At minimum pure/unit tests:

- source coverage includes workflow_execution_stats lane.
- stats lane unavailable does not fail snapshot.
- success rate formatting.
- workflow stats can upgrade evidence but not protected/destructive classification.

If adding store trait method, add PG query unit only if repo has a pattern; otherwise pure tests plus build are acceptable.

## 验收

- `cargo test -p missiond-daemon handlers::comm::capability_usage::tests`
- `cargo test -p missiond-daemon`
- `cargo test -p missiond-core --lib` if core touched
- `cargo test -p missiond-mcp --lib` if MCP touched
- `cargo build --workspace`
- `node scripts/check-architecture-lisp.mjs --all-v2`
- `git diff --check`

## 交付报告

说明：

- Data source and query.
- New source_coverage lane.
- Whether classification uses the stats or only reports evidence.
- Any read-only limitations.

