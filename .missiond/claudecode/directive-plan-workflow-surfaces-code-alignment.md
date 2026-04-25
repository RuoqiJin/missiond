# MissionD v2 Code Alignment Task: Directive / Plan / Workflow Manager Surfaces

请按 MissionD v2 Lisp 架构做代码同构：实现 `mission_directive` / `mission_plan` / `mission_workflow` 三个管理面，把已经存在的 DirectiveLayerStore 接起来。

只做代码同构，不重新设计架构，不修改 `.missiond/v2/*.lisp`。当前 Lisp 是工作树里的最新设计，请先读这些锚点：

- `/Users/jinchen/Projects/missiond/.missiond/v2/intent-memory.lisp` :: `module directive-layer`
- `/Users/jinchen/Projects/missiond/.missiond/v2/intent-intent-layer.lisp` :: `directive-plan-workflow-pipeline`
- `/Users/jinchen/Projects/missiond/.missiond/v2/intent-flow.lisp` :: `F-directive-plan-workflow-compile`
- `/Users/jinchen/Projects/missiond/.missiond/v2/intent-tools.lisp` :: `mission_directive` / `mission_plan` / `mission_workflow`

## Parallel Scope

This task owns the directive/plan/workflow manager lane and will likely touch shared MCP registration files:

- `crates/missiond-core/src/db/**` DirectiveLayerStore APIs if gaps exist
- `crates/missiond-daemon/src/handlers/knowledge/**` or a new intent-layer handler module
- `crates/missiond-mcp/src/tools/**`
- `crates/missiond-daemon/src/handlers/mod.rs`
- `crates/missiond-mcp/src/tools/mod.rs`

Do not run this simultaneously with other tasks that add new MCP tools, especially `mission_global_instruction`, unless you are prepared to merge registration conflicts.

## Goals

1. Implement `mission_directive` actions:
   - `compile`
   - `list`
   - `get`
   - `approve`
   - `archive`
   - `version_chain`
2. Implement `mission_plan` actions:
   - `compile`
   - `list`
   - `get`
   - `by_task`
   - `approve`
   - `mark`
   - `supersede`
   - `execute`
   - `record_evidence`
3. Implement `mission_workflow` actions:
   - `list`
   - `get`
   - `match`
   - `apply`
   - `distill`
   - `record_execution`
   - `compile_methodology`
   - `run_methodology`
4. Use existing DirectiveLayerStore / Pg store APIs where present. If an API is missing, add the minimal trait method and PG implementation needed for the action.
5. Keep first batch pragmatic:
   - store-backed read/control actions should be full where data model exists
   - true LLM compile/distill actors may be `dry_run` / `read-only` / structured TODO if no actor exists
   - do not fake completed compiler behavior
6. `execute` should route to existing surfaces only where safe:
   - `mission_execution`
   - `mission_task_delegate`
   - `mission_flow_run`
   - otherwise return explicit `not_implemented` with next step
7. `compile_methodology` should prefer existing YAML flow loader/compiler conventions if present; otherwise produce dry-run preview and explain missing compiler.

## Non-Goals

- Do not implement Forge-side generation in this task.
- Do not invent a new database schema unless existing directive/plan/workflow tables are truly missing.
- Do not modify `.missiond/v2/*.lisp`.
- Do not pretend LLM compiler actors are done if only store plumbing is implemented.

## Acceptance

- `cargo build --workspace`
- `cargo test -p missiond-core --lib`
- `cargo test -p missiond-daemon`
- `cargo test -p missiond-mcp`
- `node scripts/check-architecture-lisp.mjs --all-v2`
- `git diff --check`

Add focused tests where practical:

- each MCP tool is registered and unique
- required action validation
- store-backed list/get paths
- dry-run responses for actor-pending compile/distill paths
- plan `execute` rejects unsafe/unknown target cleanly

## Deliverables

- List modified files.
- For every action, mark status: `full` / `dry-run` / `read-only` / `not-implemented`.
- State whether any migration was added.
- State remaining actor-pending work.
