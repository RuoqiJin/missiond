# MissionD wave11 correction: live-tree gap fix

使用 agent-team 提高效率，但必须按 ownership 拆分，避免互相覆盖。

只做代码同构与修正，不重新设计架构。当前 `.missiond/v2/*.lisp` 是工作树里的最新设计；本任务只在代码侧追平或修正明显偏差。开始前先读：

- `.missiond/v2/intent-flow.lisp`
- `.missiond/v2/intent-intent-layer.lisp`
- `.missiond/v2/intent-tools.lisp`
- `.missiond/v2/intent-worker.lisp`
- `.missiond/v2/intent-memory.lisp`
- `.missiond/v2/architecture-dsl.lisp`

背景：A+B 组报告说 8 项完成，但当前 live worktree 里有 3 项报告产物缺失或只存在旧实现，另有 3 个 code-quality/contract 偏差需要修正。

## 当前必须修的 6 件事

### 1. 恢复 ExecutionEvent dispatch metadata

当前 live tree 里 `crates/missiond-core/src/event/events/execution.rs` 的 `ExecutionEvent::Opened` 仍只有：

- `execution_id`
- `parent_design`
- `scope`
- `owner`
- `path`

需要按 Lisp 的 workstation dispatch record 补 3 个可选字段：

- `dispatch_strategy: Option<String>`
- `target_project: Option<String>`
- `requested_cwd: Option<String>`

要求：

- 字段必须 `#[serde(default, skip_serializing_if = "Option::is_none")]`，保证 legacy JSON 反序列化兼容，且未传值时 wire 尽量保持旧格式。
- 更新 `crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs` 的 `ExecutionEvent::Opened` 发布路径，把已存在的 companion-log metadata 同步进 event。
- 不破坏现有 companion log meta 持久化；已有 `dispatch_strategy / target_project / requested_cwd` 逻辑要复用。
- 增加/更新 serde round-trip tests，覆盖 old JSON without fields + new JSON with fields。

### 2. 恢复 mission_flow_run longest-prefix project root

当前 live tree 的 `crates/missiond-daemon/src/handlers/compute/flow_run.rs` 仍是 custom resolver：cwd 只当绝对路径使用，project/target_project 只 exact registry id；没有 longest-prefix registry source。

需要对齐 `intent-worker.lisp :: project-root-spawn-cwd` 与 slot orchestrator：

- 当 `cwd` 位于某个已注册项目目录下，解析为“最长前缀匹配”的项目根。
- response/source 中暴露 source，例如 `registry_longest_prefix` 或等价稳定枚举。
- `flow_path` 为相对路径时仍必须有 resolved project root；不允许进程 CWD 隐式解析。
- `project` / `target_project` exact registry id 仍支持。
- 不破坏旧行为：无 project/cwd 时仍可搜索 `$MISSIOND_HOME/flows` core flow。
- 增加 tests：
  - cwd inside registered project resolves to project root
  - nested projects choose longest prefix
  - unresolved cwd + relative flow_path rejected
  - old core flow lookup still works without project root

### 3. 恢复 capability_usage workflow execution stats lane

当前 live tree 的 `crates/missiond-daemon/src/handlers/comm/capability_usage.rs` 仍只有 5 个 source lanes：

- conversation_tool_calls
- board_tasks_flow_template
- event_log_flow_events
- lisp_semantic_hints
- review_sidecar

需要加入第 6 lane：

- `workflow_execution_stats`

要求：

- exact-name mapping only，不做 fuzzy matching。
- DB/query 失败时 lane status=`unavailable`，不挂主响应。
- 只 upgrade evidence，不改变 destructive classification 规则。
- source_coverage.sources 必须稳定包含 6 个 lane。
- 增加 tests：source coverage includes all six lanes；DB unavailable path does not fail snapshot/report/candidates。

### 4. 修正 atomic temp path 并发风险

当前有两处 atomic writer 使用固定临时路径：

- `crates/missiond-daemon/src/handlers/knowledge/file_artifacts.rs`
- `crates/missiond-daemon/src/handlers/knowledge/workflow.rs` 的 generated YAML writer

```rust
let tmp = path.with_extension("tmp.write");
```

这在同一 artifact 并发写入时会互相踩 temp file。请改为同目录唯一 temp 路径，例如包含 pid + timestamp/nanos + thread-safe counter；不引入新依赖也可以。

要求：

- 仍在 target 同目录，保证 rename 同文件系统。
- 成功后不留 temp file。
- rename 失败时只清理本次生成的 temp file。
- 增加 test：两次连续写入使用不同 temp naming helper；如能低成本做并发测试更好。
- 不要留下任何新的 `with_extension("tmp.write")` 或固定 `.tmp.write` 路径。

### 5. 统一 plan/workflow file-first writer 的 project-root resolver

当前 directive writer 已接近正确，但 plan/workflow writer 仍有 custom resolver：

- `plan.rs::resolve_write_file_project_root` 会 fallback 到 `std::env::current_dir()`。
- `workflow.rs::resolve_write_project_root` 允许相对 cwd join process CWD，并且无 project 时 fallback 到 process CWD。

这违反 project-root contract。请统一到 `crate::slot_orchestrator::project_root::resolve_target_project_root` 或与它等价的 helper。

要求：

- `write_file=true` 时，必须由 `project` / `target_project` / absolute `cwd` 成功解析到 canonical project root。
- relative cwd 必须拒绝，不得 join process CWD。
- 不允许 process CWD fallback。
- 对 plan：如果在 DB insert 前能发现 resolver 失败，返回 structured error 即可；如果 DB 已写后发现文件写失败，则 status=`partial`。
- 对 workflow：DB 已写后 resolver/file write 失败必须 status=`partial` + `file_write_error`，但不得落到 process CWD。
- 增加 tests 覆盖 plan/workflow relative cwd rejected、missing project signal rejected/no process cwd fallback、registered project id success。

### 6. 清掉新引入 warning

当前 `cargo build --workspace` 出现一个本 wave 新 warning：

- `crates/missiond-daemon/src/handlers/knowledge/plan.rs` unused import `CompileReviewGateRequest`

删除 unused import 或实际使用它。不要碰无关历史 warning。

## 验收

必须跑：

- `cargo test -p missiond-core --lib`
- `cargo test -p missiond-daemon`
- `cargo test -p missiond-mcp --lib`
- `cargo build --workspace`
- `node scripts/check-architecture-lisp.mjs --all-v2`
- `git diff --check`

额外用 `rg` 自检：

```bash
rg -n "dispatch_strategy|target_project|requested_cwd" crates/missiond-core/src/event/events/execution.rs crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs
rg -n "RegistryLongestPrefix|registry_longest|longest.*prefix|resolve_target_project_root" crates/missiond-daemon/src/handlers/compute/flow_run.rs
rg -n "workflow_execution_stats" crates/missiond-daemon/src/handlers/comm/capability_usage.rs
rg -n "current_dir\\(|tmp\\.write|with_extension\\(\"tmp.write\"\\)" crates/missiond-daemon/src/handlers/knowledge/file_artifacts.rs crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-daemon/src/handlers/knowledge/workflow.rs
```

## Git handoff contract

本任务完成并验收通过后，请直接 commit 自己的工作，避免并行工位后续 stash/pop 或清理冲突时把成果回退。

提交规则：

- 只 stage 本任务 ownership 范围内实际修改的文件，不要 `git add .`。
- 提交前必须运行 `git diff --cached --name-only`，确认 staged 文件只属于本任务。
- 如果发现 staged 里有别人的文件，先 unstage 那些文件，再 commit。
- commit message 建议：
  `fix(wave11): restore live-tree gaps for dispatch/root/stats and file writers`
- 如果因为并行工位导致 git index lock 或冲突，停止并在报告里说明，不要用 `git checkout` / `git reset` / stash 去处理别人的文件。

最后交付：

- 列出修改文件。
- 标明 6 件事每件的状态。
- 明确是否还有 read-only/dry-run/stub。
- 写出 commit hash；如果没 commit，明确原因。
