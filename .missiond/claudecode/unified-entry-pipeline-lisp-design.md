# MissionD v2 Lisp 架构任务：统一入口 message → alignment → plan → execution → workflow

工作目录：`/Users/jinchen/Projects/missiond`

你是 ClaudeCode。请只做 Lisp 架构设计，不做 Rust 代码同构。

## 背景

当前 MissionD 已经完成第一批代码同构：

- `mission_execution`：12-action execution manager，已能协调 execution companion log。
- `mission_capability_usage`：tool/flow usage monitor，已能产出 usage snapshot/report/candidates。
- `mission_directive` / `mission_plan` / `mission_workflow`：管理面已实现 partial。
  - directive compile：dry-run；`persist=true` 可写 draft row。
  - plan compile：dry-run；`persist=true` 可写 draft row。
  - plan execute：当前只返回 `next_call` bridge descriptor，不自动内部执行。
  - workflow distill / compile_methodology：dry-run / preview；真正 distiller/YAML emitter pending。
- `mission_global_instruction`：read/edit full，reload manual。
- project-root spawn cwd contract：已实现。
- ExecutionEvent / CapabilityUsage ObservabilityEvent：已实现。

用户期望 MissionD 的长期运作方式不是“某个 client 直接 MCP 调工位”，而是统一流程：

```text
message
  -> intent-alignment.lisp
  -> human/Codex review edits until approved
  -> PLAN.lisp
  -> human/Codex review edits until approved
  -> MissionD internal execution
  -> evidence collection
  -> workflow.lisp distillation when reusable
```

你的任务是把这个统一入口 pipeline 作为 MissionD 的架构主线补到 v2 Lisp。

## 只允许修改

只修改这些 Lisp 架构文件：

- `.missiond/v2/intent-flow.lisp`
- `.missiond/v2/intent-intent-layer.lisp`
- `.missiond/v2/intent-tools.lisp`
- `.missiond/v2/intent-memory.lisp`
- `.missiond/v2/intent.lisp`

如确有必要，可少量同步：

- `.missiond/v2/intent-worker.lisp`
- `.missiond/v2/intent-system-layer.lisp`

不要修改 Rust / SQL / JS / shell / `.missiond/intent-mcp-defs.lisp`。
不要 stage，不要 commit。

## 设计原则

1. 不要新增重复 flow。
   先读现有 `F-intent-alignment-plan-execution-loop` 和 `F-directive-plan-workflow-compile`，在原有结构上升级。

2. 不要假装已实现。
   清楚标注：
   - implemented / code-aligned
   - code-aligned partial
   - architecture-designed
   - code-alignment pending
   - dry-run / read-only / manual

3. 统一入口优先复用现有 surface。
   当前优先把 `mission_directive(action=compile, source=message|architecture_lisp_delta|user_request)` 设计成统一 message intake 的管理入口。不要随意设计新 MCP tool；如果认为未来需要 `mission_message` 或 `mission_invoke`，只能标为 future candidate，并说明为什么当前不需要。

4. 文件优先，DB 镜像。
   `intent-alignment.lisp` / `PLAN.lisp` / `workflow.lisp` 是 human/agent review 的 SSOT；DB `directive/plan/workflow` 是可查询镜像和状态管理面。

5. 审阅 gate 是一等状态。
   alignment 和 plan 都必须有 review/approval gate，不能从 LLM 产物直接进入执行。

## 必须改的架构内容

### 1. `.missiond/v2/intent-flow.lisp`

升级现有 `F-intent-alignment-plan-execution-loop`，使其成为统一入口 canonical flow。

必须表达完整阶段：

1. `message-intake`
   - 来源：用户 message / external MCP client / board task / architecture Lisp delta。
   - 当前 surface：`mission_directive(action=compile)`。
   - 输出：directive draft 或 file-first alignment request。

2. `intent-alignment-authoring`
   - 生成 `.missiond/alignment/<topic>/intent-alignment.lisp`。
   - 执行方式可以是 direct LLM，也可以是常驻 ClaudeCode 工位。
   - 当前状态：architecture-designed; actor/worker pending。

3. `alignment-review-gate`
   - human/Codex 修改 alignment 到满意。
   - 状态从 draft/reviewing 到 approved/rejected/superseded。
   - 不通过 gate 不允许生成 PLAN。

4. `plan-authoring`
   - 读取 approved `intent-alignment.lisp`。
   - 生成 `.missiond/plans/<topic>/PLAN.lisp`。
   - 可镜像到 `mission_plan` draft row。
   - 当前 `mission_plan(action=compile)` 是 dry-run/draft，不是真正 LLM planner。

5. `plan-review-gate`
   - human/Codex 修改 PLAN 到满意。
   - approved PLAN 才能执行。

6. `execution-runner`
   - 目标：未来由 MissionD 内部调用 `mission_execution` / `mission_task_delegate` / `mission_flow_run`。
   - 当前实现：`mission_plan(action=execute)` 只返回 `next_call` descriptor；自动 runner pending。
   - 必须写清楚“不是 client 直接调工位，而是 MissionD plan-runner 内部调度”。

7. `evidence-collection`
   - 收集 git diff / tests / tool_calls / event_log / execution companion log / deviations / decisions / completions。
   - 写 plan evidence sidecar 或 plan evidence record。

8. `workflow-distillation`
   - 成功计划可进入 `workflow.lisp` / workflow table。
   - 当前 `mission_workflow(action=distill)` dry-run/draft；真正 distiller pending。

同时更新 `tool-backed-flows-index`：

- `mission_directive` 指向统一入口 directive/alignment branch。
- `mission_plan` 指向 plan review + execution-runner branch。
- `mission_workflow` 指向 distillation branch。
- `mission_execution` 仍指向 execution log governance，但要标为 unified pipeline 的 execution substrate。

### 2. `.missiond/v2/intent-intent-layer.lisp`

补一个明确 section/path，例如：

- `unified-entry-pipeline`
- 或升级现有 `alignment-plan-workflow-loop`

必须定义这些逻辑角色：

- `message-intake-manager`
- `alignment-author`
  - mode A: direct LLM
  - mode B: resident ClaudeCode slot
- `alignment-review-gate`
- `plan-compiler`
- `plan-review-gate`
- `plan-runner`
- `evidence-collector`
- `workflow-distiller`

状态要求：

- `message-intake-manager`: current surface partial via `mission_directive`
- `alignment-author`: architecture-designed, code-alignment pending
- `plan-compiler`: architecture-designed, code-alignment pending
- `plan-runner`: architecture-designed, code-alignment pending
- `evidence-collector`: architecture-designed, partial evidence sidecar exists via `mission_plan(record_evidence)`
- `workflow-distiller`: architecture-designed, code-alignment pending

必须强调：MissionD 统一流程的目标是可自动化、可 flow 化、可复用，不依赖某个交互 client 私有调度能力。

### 3. `.missiond/v2/intent-tools.lisp`

不要新增 tool。

在已有 implemented surfaces 里补 cross-ref：

- `mission_directive`
  - 作为当前统一入口 message intake / directive draft 管理面。
  - compile 仍是 dry-run / draft persistence，不是最终 LLM alignment author。

- `mission_plan`
  - 作为 PLAN.lisp / plan row 管理面。
  - execute 当前是 bridge descriptor；未来 plan-runner 会内部消费。

- `mission_workflow`
  - 作为 workflow distillation / methodology compile 管理面。

- `mission_execution`
  - 作为统一 pipeline execution substrate。

如加入 future candidate，必须明确是 future，不计入当前 83 tool。

### 4. `.missiond/v2/intent-memory.lisp`

加强 directive-layer 的 file-first artifact 契约：

- `.missiond/alignment/<topic>/intent-alignment.lisp`
  - maps-to directive
  - status lifecycle: draft / reviewing / approved / rejected / superseded
  - review gate owner: human/Codex

- `.missiond/plans/<topic>/PLAN.lisp`
  - maps-to plan
  - status lifecycle: draft / reviewing / approved / executing / succeeded / failed / superseded

- `.missiond/workflows/<topic>.lisp`
  - maps-to workflow
  - created after repeated success or explicit human mark reusable

补 evidence 契约：

- `.missiond/v2/plans/<plan_id>.evidence.json` 当前已由 `mission_plan(record_evidence)` 写。
- 后续可升级为 DB JSONB 或 workflow distillation input。

### 5. `.missiond/v2/intent.lisp`

同步导航摘要：

- intent-layer canonical status 要提到 unified entry pipeline architecture-designed。
- flow canonical status 要提到 `message → alignment → plan → execution → workflow`。
- tools summary 保持当前 83 tools，不要回退。
- memory summary 保持 v0.5.5，补 file-first unified pipeline artifact。

## 验收命令

必须运行：

```bash
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check
```

不要运行 cargo，因为本任务不改 Rust。

## 交付报告

完成后请报告：

- 修改了哪些 Lisp 文件。
- 你如何升级了统一入口 pipeline。
- 哪些能力是已实现 / partial / pending。
- 是否新增任何 future candidate tool；如有，为什么没有计入当前 83。
- 验收命令结果。

不要 stage，不要 commit。
