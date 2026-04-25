# MissionD v2 code-alignment: generated flow loader / mission_flow_run discoverability

使用 agent-team 提高效率，但写入范围必须保持清晰。只做代码向 Lisp 对齐，不重新设计架构，不修改 `.missiond/v2/*.lisp`，不 stage，不 commit。

## 目标

上一批 `mission_workflow(action=compile_methodology, persist=true)` 已把 generated YAML 写到：

`<project_root>/.missiond/generated/flows/<flow_id>.yaml`

但当前 flow loader 只搜索 `$MISSIOND_HOME/flows/<flow_id>.yaml`。本任务把 generated flows 接入 `mission_flow_run` 的可发现/可运行路径：

- `mission_flow_run(action=list)` 能列出 core flows + project generated flows。
- `mission_flow_run(action=run, flow_id=..., project/target_project=...)` 能优先或补充搜索 `<project_root>/.missiond/generated/flows`。
- 支持显式 `flow_path` 运行一个 YAML 文件，便于 methodology compiler 输出后直接验证。
- 不新增 MCP tool，不新增 migration，不改 Lisp。

Lisp 锚点：

- `.missiond/v2/intent-flow.lisp :: F-methodology-to-executable-compile :: s5/s6`
- `.missiond/v2/intent-tools.lisp :: mission_workflow compile_methodology/run_methodology`
- `.missiond/v2/intent-tools.lisp :: mission_flow_run`

## 写入范围

主要 ownership：

- `crates/missiond-daemon/src/engine/flow/loader.rs`
- `crates/missiond-daemon/src/handlers/compute/flow_run.rs`
- `crates/missiond-mcp/src/tools/compute/flow_run.rs`

允许只在必要时读：

- `crates/missiond-daemon/src/slot_orchestrator/project_root.rs`
- `crates/missiond-daemon/src/handlers/knowledge/workflow.rs`

不要改：

- `.missiond/v2/*.lisp`
- `plan.rs`
- `workflow.rs` unless absolutely necessary for compatibility
- DB migrations

## 期望行为

### 1. Loader search paths

Add helper(s) in `engine::flow::loader`:

- core flow dir: `$MISSIOND_HOME/flows`
- generated project flow dir: `<project_root>/.missiond/generated/flows`

Suggested APIs:

- `load_flow(flow_id)` remains backward compatible and searches only existing default location or delegates to a new helper with no project root.
- `load_flow_with_project(flow_id, Option<&Path>)`
- `load_flow_from_path(path)`
- `list_flows_with_project(Option<&Path>)`

Search order:

1. explicit `flow_path` if provided
2. project generated flows if project root resolved
3. `$MISSIOND_HOME/flows`

Response should include `flow_source` / `flow_path` when possible.

### 2. mission_flow_run schema

Extend schema with optional:

- `project`
- `target_project`
- `cwd`
- `flow_path`

For `action=list`, accept project/cwd to include generated flows.

For `action=run`:

- `flow_id` still works as before.
- If `flow_path` supplied, `flow_id` may be omitted and should be read from YAML after load.
- If project cannot resolve, do not break old core-flow behavior; return structured warning/source coverage if useful.

### 3. Project root resolution

Reuse existing resolver if straightforward. If resolver is tightly coupled to async state, keep this task small:

- path-like `project` / `target_project` / `cwd` can be canonicalized directly,
- registry project id support can be marked TODO if no lightweight API exists,
- do not duplicate a complex project registry.

Be honest in response fields:

- `project_root_status: resolved|unresolved|not_requested`
- `searched_paths: [...]`

### 4. Tests

Add unit tests:

- default `load_flow` still loads existing example/core flow behavior.
- generated dir search finds `<project_root>/.missiond/generated/flows/foo.yaml`.
- explicit flow_path wins.
- list merges core + generated without duplicates.
- missing flow error includes searched paths.
- `mission_flow_run` schema exposes project/target_project/cwd/flow_path.

Run acceptance:

```bash
cargo test -p missiond-daemon engine::flow::loader::tests
cargo test -p missiond-daemon handlers::compute::flow_run::tests
cargo test -p missiond-daemon
cargo test -p missiond-mcp --lib
cargo build --workspace
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check
```

If `handlers::compute::flow_run::tests` does not exist, add focused pure/unit tests there or document why loader tests are sufficient.

## 交付报告

请列：

- 修改文件
- loader search order
- list/run response additions
- project root resolution status and limitations
- backward compatibility for old `$MISSIOND_HOME/flows`
- 测试结果
- 明确说明未修改 `.missiond/v2/*.lisp`、未 stage、未 commit
