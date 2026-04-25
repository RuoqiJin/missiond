# MissionD wave 11 code-alignment: workflow.lisp file-first writer

使用 agent-team 提高效率，但写入范围必须保持清晰。只做代码向 Lisp 对齐，不重新设计架构，不修改 `.missiond/v2/*.lisp`，不 stage，不 commit。

依赖：优先等 `.missiond/claudecode/wave11-file-artifact-foundation-code-alignment.md` 完成后再执行。

## 目标

让 `mission_workflow(action=distill, persist=true, write_file=true)` 在写 workflow DB row 的同时，写入：

`<project_root>/.missiond/workflows/<topic>.lisp`

此文件是 reusable workflow 的 file-first SSOT。DB row 是 match/query/status mirror。

## Lisp 锚点

- `.missiond/v2/intent-flow.lisp :: F-intent-alignment-plan-execution-loop :: s8 workflow-distillation`
- `.missiond/v2/intent-memory.lisp :: directive-layer :: file-first-artifacts :: workflow-methodology-artifact`
- `.missiond/v2/intent-tools.lisp :: mission_workflow`

## 写入范围

允许修改：

- `crates/missiond-daemon/src/handlers/knowledge/workflow.rs`
- `crates/missiond-mcp/src/tools/knowledge/workflow.rs`

只读参考：

- `crates/missiond-daemon/src/handlers/knowledge/file_artifacts.rs`

不要修改：

- `directive.rs`
- `plan.rs`
- DB migrations
- `.missiond/v2/*.lisp`

## 行为要求

新增 optional input fields for `action=distill`：

- `write_file: bool` default false
- `topic: string` or reuse `name` as topic when topic omitted
- `project` / `target_project` / `cwd`: resolve project root
- `overwrite_file: bool` default false

Distill behavior:

- `persist=false, write_file=true` refuses with structured `INVALID_PARAM`.
- `persist=true, write_file=false` unchanged.
- `persist=true, write_file=true` writes workflow lisp containing:
  - `workflow_sexp`;
  - metadata with workflow_id, source_plan_id, distill_mode, compiler_model, generated_at;
  - match_rules as a compact lisp/json block or comment, but do not invent a new parser requirement.

Failure semantics:

- Existing file without overwrite refuses.
- If DB insert succeeds and file write fails, return partial with workflow_id/file_write_error.

Do not alter `compile_methodology` / `run_methodology` except for shared helper imports if necessary.

## Tests

At minimum:

- write_file requires persist.
- topic/name path resolution.
- existing file refusal.
- file content includes workflow_id/source plan metadata.
- existing workflow tests still pass.

## 验收

- `cargo test -p missiond-daemon handlers::knowledge::workflow::tests`
- `cargo test -p missiond-daemon`
- `cargo test -p missiond-mcp --lib`
- `cargo build --workspace`
- `node scripts/check-architecture-lisp.mjs --all-v2`
- `git diff --check`

## 交付报告

说明：

- 写入路径。
- response fields。
- partial failure behavior。
- 是否影响 methodology compiler / runner。

