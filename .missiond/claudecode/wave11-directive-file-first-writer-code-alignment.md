# MissionD wave 11 code-alignment: directive file-first writer

使用 agent-team 提高效率，但写入范围必须保持清晰。只做代码向 Lisp 对齐，不重新设计架构，不修改 `.missiond/v2/*.lisp`，不 stage，不 commit。

依赖：优先等 `.missiond/claudecode/wave11-file-artifact-foundation-code-alignment.md` 完成后再执行。

## 目标

让 `mission_directive(action=compile, persist=true, write_file=true)` 在写 directive DB draft 的同时，写入 file-first artifact：

`<project_root>/.missiond/alignment/<topic>/intent-alignment.lisp`

这不是替代 DB row。Lisp 约定是 file-first SSOT + DB mirror。当前任务只补 alignment artifact 写入。

## Lisp 锚点

- `.missiond/v2/intent-flow.lisp :: F-intent-alignment-plan-execution-loop :: s2 intent-alignment-authoring`
- `.missiond/v2/intent-memory.lisp :: directive-layer :: file-first-artifacts :: intent-alignment-artifact`
- `.missiond/v2/intent-tools.lisp :: mission_directive`

## 写入范围

允许修改：

- `crates/missiond-daemon/src/handlers/knowledge/directive.rs`
- `crates/missiond-mcp/src/tools/knowledge/directive.rs`

只读参考：

- `crates/missiond-daemon/src/handlers/knowledge/file_artifacts.rs`
- `crates/missiond-daemon/src/slot_orchestrator/project_root.rs`
- `crates/missiond-daemon/src/handlers/compute/flow_run.rs` (project/root arg pattern)

不要修改：

- `plan.rs`
- `workflow.rs`
- DB migrations
- `.missiond/v2/*.lisp`

## 行为要求

新增 optional input fields：

- `write_file: bool` default false
- `topic: string` required when `write_file=true`
- `project` / `target_project` / `cwd`: resolve project root
- `overwrite_file: bool` default false

Compile behavior:

- `persist=false, write_file=true` should refuse with `INVALID_PARAM`; file write must be tied to a DB draft mirror.
- `persist=true, write_file=false` behavior unchanged.
- `persist=true, write_file=true`:
  - writes directive DB draft first or in a clearly documented order;
  - writes alignment file with compiled sexp plus metadata comment/form containing directive_id, version, compiler_mode, source, generated_at;
  - if file exists and overwrite_file=false, return structured error and do not pretend success.

Error semantics:

- If DB insert succeeds but file write fails, response must be honest: `status="partial"` or `file_write_error`, include directive_id/version so caller can repair.
- Do not delete DB row on file error unless a transaction already exists and is safe. If no transaction, explicitly report partial.

## Tests

Add focused unit tests for pure helpers or testable functions:

- write_file requires persist.
- topic required when write_file=true.
- response shape includes artifact_path and artifact_status on success.
- existing file without overwrite returns structured refusal.

If full handler tests are too heavy, extract small pure functions and test them.

## 验收

- `cargo test -p missiond-daemon handlers::knowledge::directive::tests`
- `cargo test -p missiond-daemon`
- `cargo test -p missiond-mcp --lib`
- `cargo build --workspace`
- `node scripts/check-architecture-lisp.mjs --all-v2`
- `git diff --check`

## 交付报告

必须说明：

- `write_file` full/partial 状态。
- 失败时是否可能 DB row 已写但 file 未写。
- 文件路径约定。
- 是否覆盖 existing file。

