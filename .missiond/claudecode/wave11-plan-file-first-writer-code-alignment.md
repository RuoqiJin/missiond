# MissionD wave 11 code-alignment: PLAN.lisp file-first writer

使用 agent-team 提高效率，但写入范围必须保持清晰。只做代码向 Lisp 对齐，不重新设计架构，不修改 `.missiond/v2/*.lisp`，不 stage，不 commit。

依赖：优先等 `.missiond/claudecode/wave11-file-artifact-foundation-code-alignment.md` 完成后再执行。

## 目标

让 `mission_plan(action=compile, persist=true, write_file=true)` 在写 plan DB row 的同时，写入：

`<project_root>/.missiond/plans/<topic>/PLAN.lisp`

保持 DB row 为镜像/状态面，PLAN.lisp 为 review 边界。

## Lisp 锚点

- `.missiond/v2/intent-flow.lisp :: F-intent-alignment-plan-execution-loop :: s4 plan-authoring`
- `.missiond/v2/intent-memory.lisp :: directive-layer :: file-first-artifacts :: plan-artifact`
- `.missiond/v2/intent-tools.lisp :: mission_plan`

## 写入范围

允许修改：

- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`
- `crates/missiond-mcp/src/tools/knowledge/plan.rs`

只读参考：

- `crates/missiond-daemon/src/handlers/knowledge/file_artifacts.rs`

不要修改：

- `directive.rs`
- `workflow.rs`
- DB migrations
- `.missiond/v2/*.lisp`

## 行为要求

新增 optional input fields for `action=compile`：

- `write_file: bool` default false
- `topic: string` required when `write_file=true`
- `project` / `target_project` / `cwd`: resolve project root
- `overwrite_file: bool` default false

Compile behavior:

- `persist=false, write_file=true` refuses with structured `INVALID_PARAM`.
- `persist=true, write_file=false` unchanged.
- `persist=true, write_file=true` writes `PLAN.lisp` containing:
  - compiled plan sexp;
  - metadata with plan_id, version, board_task_id, source_directive_id, compiler_mode, compiled_from, generated_at;
  - current status `awaiting_approval` or `draft` matching DB state.

Failure semantics:

- If DB succeeds and file write fails, return `status="partial"` with `plan_id/version/file_write_error`.
- Existing file without `overwrite_file=true` refuses, no silent overwrite.

Do not change `action=execute` behavior except if needed to surface plan_file path in response when available. Avoid touching plan-runner auto-selection internals.

## Tests

At minimum:

- write_file requires persist.
- topic required.
- compiled file content contains plan_id/board_task_id metadata.
- overwrite refusal.
- existing plan-runner tests still pass.

## 验收

- `cargo test -p missiond-daemon handlers::knowledge::plan::tests`
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
- plan-runner execute 是否保持兼容。

