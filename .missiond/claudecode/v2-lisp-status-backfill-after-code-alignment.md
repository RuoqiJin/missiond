# MissionD v2 Lisp backfill: align statuses after code-alignment batches

使用常驻 Lisp 架构会话执行；不要改 Rust/SQL/JS，不 stage，不 commit。只更新 `.missiond/v2/*.lisp` 的架构状态、cross-ref、version note，使 Lisp 重新成为当前代码真相的架构 SSOT。

## 背景

以下代码同构批次已经完成并提交：

- `0a3ffe0 feat(intent): add compiler actor v0 surfaces`
  - directive-compiler actor v0
  - plan-compiler actor v0
  - workflow-distiller actor v0
- `3ed14fc feat(workflow): align execution metadata and usage evidence`
  - mission_execution dispatch_strategy companion record
  - mission_workflow methodology compiler/runner v0
  - mission_capability_usage semantic evidence v1

当前 Lisp 里还有若干旧状态，例如 `actor pending`、`distill dry-run`、`compile_methodology dry-run`、`execute returns next_call` 等，需要按代码现状更新。只做状态回填和 cross-ref 修正，不重新设计。

## 写入范围

允许修改：

- `.missiond/v2/intent.lisp`
- `.missiond/v2/intent-flow.lisp`
- `.missiond/v2/intent-intent-layer.lisp`
- `.missiond/v2/intent-tools.lisp`
- `.missiond/v2/intent-memory.lisp`
- `.missiond/v2/intent-worker.lisp` 仅当 workstation dispatch 状态需要同步

不要修改：

- Rust / SQL / JS / Cargo files
- `.missiond/intent-mcp-defs.lisp`
- drafts 除非 checker 需要

## 必须反映的代码事实

### 1. directive compiler

`mission_directive(action=compile)`:

- default / `compiler_mode="dry_run"` 仍 dry-run，不调 LLM。
- `compiler_mode="sonnet"` 已 code-aligned，调 Sonnet interactive lane 生成 directive sexp，验证 fenced block / parens / allowed top-level head。
- `persist=true` 写 draft，等待人工 review/approve。

更新锚点：

- `intent-flow.lisp :: F-intent-alignment-plan-execution-loop :: s2/s3`
- `intent-intent-layer.lisp :: unified-entry-pipeline :: role alignment-author`
- `intent-tools.lisp :: mission_directive`
- `intent-memory.lisp :: directive-layer :: directive-artifact`

### 2. plan compiler + runner

`mission_plan(action=compile)`:

- default dry-run 兼容。
- `compiler_mode="sonnet"` 已 code-aligned，从 approved directive / board task 编译 plan sexp。
- `persist=true` 写 `awaiting_approval`，不自动 approve。

`mission_plan(action=execute)`:

- bridge mode 仍支持 next_call descriptor。
- internal mode 已 code-aligned，可直接 dispatch 到 `mission_execution` / `mission_task_delegate` / `mission_flow_run`。
- plan-runner 会写 evidence sidecar，并把 plan 状态推进到 executing；status update failure 会暴露 partial。
- `dispatch_strategy` 已进入 response + evidence + mission_execution forwarding。
- 仍未实现：从 PLAN.lisp DAG 自动选择 target/dispatch_strategy；这要继续标 `code-alignment pending`。

更新锚点：

- `intent-flow.lisp :: F-intent-alignment-plan-execution-loop :: s4/s5/s6/s7`
- `intent-intent-layer.lisp :: role plan-compiler / plan-runner / evidence-collector`
- `intent-tools.lisp :: mission_plan`

### 3. workflow distiller + methodology compiler

`mission_workflow(action=distill)`:

- default dry-run 保留。
- `distill_mode="sonnet"` 已 code-aligned，读取 succeeded plan + evidence sidecar，生成 workflow sexp + match_rules JSON。
- `persist=true` 写 workflow draft/template，仍需 review。

`mission_workflow(action=compile_methodology|run_methodology)`:

- `compile_methodology` default dry-run 兼容。
- `compile_mode="deterministic"` 已 code-aligned，读取 `.missiond/workflows/<name>.lisp` 或 explicit path，验证括号，提取 `(step ...)`，生成 YAML。
- `persist=true` 写 `<project_root>/.missiond/generated/flows/<flow_id>.yaml`。
- `run_methodology` 已支持 missing compiled YAML pointer、dry_run would_run、dry_run=false 执行 flow engine。
- 仍未实现：高阶 methodology semantic lifting / forge compiler / 自动 record_execution 关联。

更新锚点：

- `intent-flow.lisp :: F-methodology-to-executable-compile`
- `intent-tools.lisp :: mission_workflow`
- `intent-intent-layer.lisp :: workflow-distiller / methodology compiler`

### 4. execution dispatch metadata

`mission_execution(action=open)`:

- 接收 `dispatch_strategy` 并归一化。
- companion log meta 总写 `:dispatch-strategy`，可选 `:target-project` / `:requested-cwd`。
- list/status 可读；legacy logs 兼容。
- EventBus ExecutionEvent 未扩展 dispatch metadata；标注为 companion-log durable only / event metadata future.

更新锚点：

- `intent-tools.lisp :: mission_execution :: :workstation-dispatch-record`
- `intent-worker.lisp :: claudecode-workstation-orchestration`
- `intent-flow.lisp :: F-workstation-dispatch-policy`

### 5. capability usage semantic evidence

`mission_capability_usage`:

- semantic evidence v1 已 code-aligned。
- `merge-candidate` 不再永远为空；基于 Lisp 显式 replacement / moved-to / preferred / consolidated hints 保守生成。
- flow evidence 增加 `event_log_flow_events` read-only probe。
- `source_coverage.sources` 暴露五源：conversation_tool_calls / board_tasks_flow_template / event_log_flow_events / lisp_semantic_hints / review_sidecar。
- mark/ack 仍非 destructive；merge 需要 replacement_target 或 hint target；protected source/target 拒 destructive。

更新锚点：

- `intent-flow.lisp :: F-capability-usage-monitoring`
- `intent-intent-layer.lisp :: capability-evolution-governance`
- `intent-tools.lisp :: mission_capability_usage`
- `intent-memory.lisp :: capability-usage-read-model`

## 保守边界

不要改 83 tool 总数。

不要把仍未实现的内容写成完成：

- PLAN.lisp DAG 自动选路仍 pending。
- plan-runner 自动选择 resident-lisp/fresh-code/agent-team 仍 pending。
- alignment/plan review gate 的自动 QuestionEvent 仍 pending。
- methodology semantic lifting / forge compiler 仍 pending。
- EventBus metadata for dispatch_strategy 仍 pending。
- generated flow loader 是否全局可发现，如代码尚未接入，就不要写成完成。

## 验收

```bash
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check .missiond/v2/intent.lisp .missiond/v2/intent-flow.lisp .missiond/v2/intent-intent-layer.lisp .missiond/v2/intent-tools.lisp .missiond/v2/intent-memory.lisp .missiond/v2/intent-worker.lisp
git diff --stat -- .missiond/v2
```

## 交付报告

请列：

- 修改的 Lisp 文件
- 哪些 `actor pending/dry-run/not_implemented` 状态被更新为 code-aligned
- 哪些仍明确保留 pending
- 83 tool 总数是否保持
- checker 结果
- 明确说明未改 Rust/SQL/JS、未 stage、未 commit
