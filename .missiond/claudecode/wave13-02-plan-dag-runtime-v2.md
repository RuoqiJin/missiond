# Wave 13 / Task 02 — PLAN DAG runtime v2: ready-node concurrency + node lifecycle

你在 `/Users/jinchen/Projects/missiond` 项目根目录工作。使用 agent-team提高效率。

前置：建议先完成 Wave13 Task 01，避免 `plan_dag.rs` 冲突。

目标：把 PLAN DAG scheduler 从 v1 顺序执行升级到 v2 runtime：ready nodes 可并发执行，节点生命周期可观测，失败/跳过逻辑清晰。仍只解释显式 `(node ...)` schema，不解释任意 Lisp。

Ownership：
- `crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs`
- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`
- 如需要事件：`crates/missiond-core/src/event/events/execution.rs`
- 如需要发布 helper：`crates/missiond-daemon/src/bus/bootstrap.rs`
- `crates/missiond-mcp/src/tools/knowledge/plan.rs` 仅限 schema/description

禁止：
- 不要修改 workflow/directive/capability_usage
- 不要修改 `.missiond/v2/*.lisp`
- 不要新增 DB migration
- 不要 `git add .`

功能要求：
1. 新增 `max_parallel_nodes` 参数，默认 1 以保持现有行为；传 >1 时并发执行 ready nodes。
2. 节点状态至少覆盖：
   - pending
   - ready
   - running
   - succeeded
   - failed
   - skipped
3. 调度规则：
   - depends-on 全 succeeded 后进入 ready
   - 上游 failed 且 failure-policy=fail-fast：停止后续
   - failure-policy=continue：无依赖的 ready nodes 可继续，下游依赖失败节点的子树 skipped
4. 每个节点 state transition 写 evidence collector。
5. 响应返回：
   - scheduler_mode
   - node_count
   - max_parallel_nodes
   - node_results[]
   - skipped_nodes[]
   - aggregate_status
6. dry_run=true 只返回 DAG 和并发计划，不 dispatch。
7. 如果扩 `ExecutionEvent`，必须 serde backward-compatible；如果不扩，则在报告里说明为何先不扩。

测试要求：
- pure scheduler tests：parallel waves、fail-fast stop、continue skips tainted subtree、max_parallel_nodes=1 等同 v1 顺序。
- response shape tests。
- `cargo test -p missiond-daemon handlers::knowledge::plan_dag::tests`
- `cargo test -p missiond-daemon handlers::knowledge::plan::tests`
- `cargo test -p missiond-daemon`
- `cargo test -p missiond-core --lib`（若动 event）
- `cargo test -p missiond-mcp --lib`
- `cargo build --workspace`
- `node scripts/check-architecture-lisp.mjs --all-v2`
- `git diff --check`

提交要求：
- scoped commit，只 stage ownership 文件。
- commit message 建议：
  `feat(plan): run ready DAG nodes with bounded concurrency`

交付报告：
- commit hash
- 并发语义说明
- failure-policy 行为矩阵
- 是否扩展 ExecutionEvent

