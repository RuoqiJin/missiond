# Wave 14 / Task 02 — PlanNodeStateChanged event + live evidence refs

你在 `/Users/jinchen/Projects/missiond` 项目根目录工作。使用 agent-team提高效率。

前置：先完成 Wave14 Task 00。可与 Task 01 并行，前提是不要改 directive/plan/workflow writer 参数；如冲突，等 Task 01 完成后再跑。

目标：把 PLAN DAG runtime v2 的 per-node lifecycle 从 sidecar-only 升级为 event-bus 可观测，并让 evidence collector 不再只能写 `EventRef::unavailable`。

Ownership：
- `crates/missiond-core/src/event/events/execution.rs`
- `crates/missiond-daemon/src/bus/bootstrap.rs`
- `crates/missiond-daemon/src/handlers/knowledge/evidence_collector.rs`
- `crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs`
- `crates/missiond-daemon/src/handlers/knowledge/plan.rs` 仅限必要调用点

禁止：
- 不要修改 `.missiond/v2/*.lisp`
- 不要修改 directive/workflow/capability_usage
- 不要新增 DB migration
- 不要 `git add .`

功能要求：
1. 扩展 `ExecutionEvent`，新增 `PlanNodeStateChanged`，字段至少：
   - `plan_id`
   - `node_id`
   - `from`
   - `to`
   - `target`
   - `dispatch_strategy`
   - `target_project`
   - `attempt`
   - `reason`
2. serde backward-compatible；已有 variants 不破坏旧 JSON。
3. `BusServices` 增加 publish helper（如已有泛型可复用则最小接入）。
4. `plan_dag.rs` 每个 node transition 发布 event；bus publish failure 不挂主 dispatch，但 response/evidence 要记录 warning。
5. evidence collector 支持 `EventRef::new("execution", "plan_node_state_changed", event_id)` 或等价稳定引用。
   - 如果 event bus 无返回 id，则用 deterministic id，如 `plan-node:<plan_id>:<node_id>:<attempt>:<from>-<to>`。
6. 保持 `EventRef::unavailable` 作为 fallback，但正常路径应尽量写 live/ref deterministic event ref。

测试要求：
- core serde round-trip for new event。
- plan_dag transition event builder tests。
- evidence entry includes event ref tests。
- `cargo test -p missiond-core --lib`
- `cargo test -p missiond-daemon handlers::knowledge::plan_dag::tests`
- `cargo test -p missiond-daemon handlers::knowledge::evidence_collector::tests`
- `cargo test -p missiond-daemon`
- `cargo test -p missiond-mcp --lib`
- `cargo build --workspace`
- `node scripts/check-architecture-lisp.mjs --all-v2`
- `git diff --check`

提交：
- scoped commit，只 stage ownership 文件。
- commit message:
  `feat(execution): publish plan node state changes`

交付报告：
- commit hash
- new event schema
- event-ref strategy
- bus failure behavior

