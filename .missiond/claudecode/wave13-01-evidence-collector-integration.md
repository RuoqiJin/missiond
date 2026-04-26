# Wave 13 / Task 01 — integrate evidence_collector into plan runner paths

你在 `/Users/jinchen/Projects/missiond` 项目根目录工作。使用 agent-team提高效率。

前置：先完成 Wave13 Task 00。

目标：Wave12 Task03 已新增 `evidence_collector.rs` typed helper，但 `plan.rs::action_execute_internal` 和 `plan_dag.rs` 仍使用 legacy JSON builder。本任务把 typed evidence collector 接入真实 plan-runner 路径，减少 dead_code，并统一 sidecar schema。

Ownership：
- `crates/missiond-daemon/src/handlers/knowledge/evidence_collector.rs`
- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`
- `crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs`
- `crates/missiond-mcp/src/tools/knowledge/plan.rs` 仅当 schema/description 必须同步时可改

禁止：
- 不要修改 `.missiond/v2/*.lisp`
- 不要修改 workflow/directive/capability_usage/agent_execution
- 不要新增 DB migration
- 不要 `git add .`

功能要求：
1. `plan.rs::action_execute_internal` 成功 dispatch 后，用 `EvidenceEntry` / `evidence_collector::append` 写 `plan_runner_dispatch` evidence。
2. `plan_dag.rs` 每个 node dispatch 用 typed evidence 写 `plan_dag_node_dispatch`，包含：
   - plan_id
   - node_id
   - target
   - dispatch_strategy
   - state transition
   - inner_result 或 inner_error
   - execution_event ref：若当前无法取得 live event id，写 `EventRef::unavailable("...")`
3. `mission_plan(action=record_evidence)` 保持旧 wire 兼容：
   - `evidence_kind/source` 都 absent 时输出与旧行为兼容
   - 任一出现时使用 typed wrap
4. 失败策略：
   - sidecar 写失败仍走现有 partial/status_update_error 语义
   - 不吞错误，不 silently 降级到 process cwd
5. 接入后尽量消除 `evidence_collector.rs` 的 dead_code warning；若某些 API 仍为未来保留，明确 `#[allow(dead_code)]` 并写理由，不能留无解释 warning。

测试要求：
- `cargo test -p missiond-daemon handlers::knowledge::evidence_collector`
- `cargo test -p missiond-daemon handlers::knowledge::plan::tests`
- `cargo test -p missiond-daemon handlers::knowledge::plan_dag::tests`
- `cargo test -p missiond-daemon`
- `cargo test -p missiond-mcp --lib`
- `cargo build --workspace`
- `node scripts/check-architecture-lisp.mjs --all-v2`
- `git diff --check`

提交要求：
- scoped commit，只 stage ownership 文件。
- commit message 建议：
  `feat(plan): route runner evidence through collector`

交付报告：
- commit hash
- 修改文件
- evidence sidecar 新旧兼容说明
- 仍保留的 warning 与理由

