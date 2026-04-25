# MissionD v2 code-alignment: workflow methodology compiler/runner v0

使用 agent-team 提高效率，但写入范围必须保持清晰。只做代码向 Lisp 对齐，不重新设计架构，不修改 `.missiond/v2/*.lisp`，不 stage，不 commit。

## 目标

把 `mission_workflow(action=compile_methodology|run_methodology)` 从 dry-run / not_implemented 推进到 methodology compiler/runner v0：

- `compile_methodology` 读取 `.missiond/workflows/<name>.lisp` 或显式 `workflow_path`，生成可执行 flow YAML 预览。
- `persist=true` 时把生成的 YAML 写到项目内可追踪位置，并记录 source hash。
- `run_methodology` 在已有 compiled YAML 时走 `mission_flow_run` 内部 dispatch；没有 compiled YAML 时返回结构化 next step。
- 不新增 MCP tool，不新增 migration，不改 Lisp。

Lisp 锚点：

- `.missiond/v2/intent-flow.lisp :: F-methodology-to-executable-compile`
- `.missiond/v2/intent-tools.lisp :: implemented-surface mission_workflow`
- `.missiond/v2/intent-intent-layer.lisp :: unified-entry-pipeline :: role workflow-distiller`

## 写入范围

主要 ownership：

- `crates/missiond-daemon/src/handlers/knowledge/workflow.rs`
- `crates/missiond-mcp/src/tools/knowledge/workflow.rs`

允许只在必要时读：

- `crates/missiond-daemon/src/handlers/comm/flow_run.rs` or existing `mission_flow_run` handler location
- `crates/missiond-daemon/src/engine/flow/loader.rs`
- existing flow YAML examples under project workflow/flow dirs

不要改：

- `.missiond/v2/*.lisp`
- `directive.rs`
- `plan.rs`
- `agent_execution.rs`
- DB migrations

## 期望行为

### 1. compile_methodology 参数扩展

Support optional fields:

- `compile_mode`: `"dry_run" | "deterministic"`; default should preserve existing dry-run behavior.
- `persist`: boolean; default false.
- `output_flow_id`: optional explicit flow id.
- `params`: optional object used only for placeholder discovery / response preview.
- `project` / `target_project`: registry id or cwd context, following current project resolution style in workflow.rs.

Do not call LLM in this task. This is deterministic compiler v0.

### 2. Deterministic compiler v0

When `compile_mode="deterministic"`:

- Read methodology Lisp source.
- Validate:
  - file exists
  - UTF-8 or lossy-safe with explicit flag
  - balanced parentheses, ignoring strings and escapes
  - non-empty top-level form
- Produce a minimal executable flow YAML that is honest about limitations:
  - `id`
  - `name`
  - `source_kind: methodology_lisp`
  - `source_path`
  - `source_hash`
  - `generated_by: mission_workflow.compile_methodology.v0`
  - a serial `steps` list derived from explicit `(step ...)` forms when possible
  - if step extraction is weak, include one safe manual-review step and mark `review_required: true`
- Never pretend semantic compilation is perfect. If only a coarse YAML can be produced, mark `compiler_status: "preview_requires_review"`.

### 3. Persist policy

When `persist=true`:

- Write generated YAML under a project-local generated dir, e.g. `.missiond/generated/flows/<flow_id>.yaml` or the repo's existing generated flow dir if one already exists.
- Use atomic write where feasible.
- Include source hash in YAML and response.
- Do not overwrite an existing generated YAML unless `overwrite=true`.
- Return `flow_id`, `flow_path`, `source_hash`, `review_required`.

### 4. run_methodology

`run_methodology` should:

- Resolve compiled YAML by `flow_id`, `flow_path`, or `name`.
- If no compiled YAML exists, return structured `MISSING_COMPILED_FLOW` with next step pointing to `compile_methodology(persist=true)`.
- If compiled YAML exists and `dry_run=true`, return `would_run` descriptor.
- If `dry_run=false`, internally dispatch to existing `mission_flow_run` handler with params.
- Do not duplicate flow engine logic.
- Record execution via existing `mission_workflow(action=record_execution)` only if the internal run returns success and the helper is safe to call without recursion. Otherwise return TODO/status field honestly.

## 测试要求

No real daemon/DB integration required unless existing helpers make it easy.

Add pure-function/unit tests:

- methodology path resolution.
- paren balance ignores strings.
- source hash stable.
- step extraction from simple `(step s1 "...")` forms.
- generated YAML contains source metadata.
- persist refuses overwrite unless `overwrite=true`.
- run_methodology missing compiled flow returns structured next step.

Run acceptance:

```bash
cargo test -p missiond-daemon handlers::knowledge::workflow::tests
cargo test -p missiond-daemon
cargo test -p missiond-mcp --lib
cargo build --workspace
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check
```

## 交付报告

请列：

- 修改文件
- compile_methodology dry_run / deterministic / persist 状态
- generated YAML 路径约定
- run_methodology dry_run / real dispatch / missing compiled flow 状态
- 哪些语义仍然需要未来 LLM/forge compiler
- 测试结果
- 明确说明未修改 `.missiond/v2/*.lisp`、未 stage、未 commit
