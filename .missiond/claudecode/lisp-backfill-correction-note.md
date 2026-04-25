# Lisp status backfill correction note

请继续当前 `.missiond/v2/intent-flow.lisp` 回填任务，但按下面约束修正已经写入的过度表述。不要重做，不要改 Rust/SQL/JS，不 stage，不 commit。

## 核心原则

Lisp 仍是架构 SSOT。当前任务只允许回填：

- implementation status
- code-aligned evidence
- implementation-targets / current implementation entry paths
- explicit pending/deviation

不要把当前代码形状反向写成架构偏好或唯一目标。如果代码未实现 file-first artifact 写入，就不要写成已 code-aligned。

## 必须修正

### 1. file-first artifact 写入不要写成已实现

当前 DB/handler manager 已实现，不等于 file-first `.lisp` artifact writer 已实现。

请修正这些 stage 的 `:writes` / `:status` / `:code-alignment`：

- `s2 intent-alignment-authoring`
  - `mission_directive compiler_mode=sonnet` 只能算 directive sexp / directive DB mirror code-aligned。
  - `.missiond/alignment/<topic>/intent-alignment.lisp` 自动写入仍 pending。

- `s4 plan-authoring`
  - `mission_plan compiler_mode=sonnet` 只能算 plan sexp / plan row code-aligned。
  - `.missiond/plans/<topic>/PLAN.lisp` file-first 自动写入/同步仍 pending。

- `s8 workflow-distillation`
  - `mission_workflow distill_mode=sonnet` 只能算 workflow table draft/template / workflow sexp / match_rules code-aligned。
  - `.missiond/workflows/<topic>.lisp` file-first 自动写入/同步仍 pending，除非代码确实写该文件。

推荐写法：把 `:writes` 区分为 architecture target 与 current code-aligned writes，例如：

- `:architecture-writes [...]`
- `:code-aligned-writes [...]`
- `:pending [...]`

或在同一字段里明确标 `(architecture-target, pending writer)`。

### 2. model-policy 不要变成 claude-sonnet 架构偏好

`s4 plan-authoring` 的 `:model-policy` 不应从 OPUS/planner-class 架构偏好收窄成 `claude-sonnet`。

请改成类似：

```lisp
:model-policy "provider alias configurable (例: OPUS-4.7-class planner / planner-class model); 不硬编码可用性; current code-aligned v0 uses claude-sonnet"
```

也就是说：架构偏好保持 provider alias / planner-class；`claude-sonnet` 只是当前 v0 implementation fact。

### 3. run_methodology 不要说成一定“内部调 mission_flow_run”

上一批代码现实是 `run_methodology` 读取 compiled YAML 后复用 flow-engine runner path；并不一定经过 `mission_flow_run` handler surface。

请把相关表述从：

- “dry_run=false 内部调 mission_flow_run”
- “run delegates to mission_flow_run”

修成更准确的：

- “dry_run=false 复用 flow-engine-v2 runner path 执行 compiled YAML”
- “mission_flow_run discoverability / generated flow loader 是后续独立 code-alignment”

如果新的 `generated-flow-loader-code-alignment` 并行任务已经完成并验收，再由后续回填更新这一点；当前不要提前写死。

### 4. implementation-targets 是当前实现入口，不是最终分文件目标

保留 `:implementation-targets`，但请在 F-intent-alignment-plan-execution-loop 或相关 stage 加一句规则：

```lisp
:implementation-target-policy "these paths name current code-aligned entry points, not final module boundaries; future code-convergence may split handlers into target files declared by Lisp"
```

这样避免把现在的大 handler 文件当作最终架构目标。

### 5. 仍 pending 的内容必须保留

不要把下面内容写成完成：

- PLAN.lisp DAG 自动选 `target` / `dispatch_strategy` / `target_project`
- alignment/plan review gate 自动 QuestionEvent
- file-first alignment/PLAN/workflow `.lisp` writer/sync
- methodology semantic lifting / forge compiler
- ExecutionEvent dispatch metadata
- generated flow loader / mission_flow_run discoverability，除非另一个代码任务已完成并已验收

## 验收

修完后运行：

```bash
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check .missiond/v2/intent-flow.lisp
git diff -- .missiond/v2/intent-flow.lisp
```

交付报告请明确：

- 修正了哪些过度表述
- 哪些 current implementation target 已记录
- 哪些 file-first writer / auto-selection / semantic compiler 仍 pending
- 未改 Rust/SQL/JS，未 stage，未 commit
