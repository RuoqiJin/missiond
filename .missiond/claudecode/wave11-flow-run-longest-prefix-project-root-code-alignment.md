# MissionD wave 11 code-alignment: mission_flow_run longest-prefix project root

使用 agent-team 提高效率，但写入范围必须保持清晰。只做代码向 Lisp 对齐，不重新设计架构，不修改 `.missiond/v2/*.lisp`，不 stage，不 commit。

## 目标

`mission_flow_run` generated flow discovery 已支持 `project / target_project / cwd`，但 project root 解析仍是本地简化版：cwd 只接受绝对存在路径作为 root，没有复用 slot/project-root 的 longest-prefix project resolver。

本任务把 `mission_flow_run` 的 cwd 解析提升到与 spawn cwd contract 一致：

- cwd 指向项目子目录时，解析为注册项目根。
- requested subdir 保留为 diagnostic metadata。
- Gemini/Codex/Claude slot spawn 规则不在本任务范围。

## Lisp 锚点

- `.missiond/v2/intent-tools.lisp :: mission_flow_run :: :pending longest-prefix cwd resolver`
- `.missiond/v2/intent-flow.lisp :: F-methodology-to-executable-compile :: generated flow loader pending richer project-root resolution`
- `.missiond/v2/intent-worker.lisp :: project-root-spawn-cwd`

## 写入范围

允许修改：

- `crates/missiond-daemon/src/handlers/compute/flow_run.rs`

只读参考：

- `crates/missiond-daemon/src/slot_orchestrator/project_root.rs`
- `crates/missiond-core/src/types/project.rs` / project registry types as needed

不要修改：

- `loader.rs` unless a type signature absolutely requires it
- `plan.rs`
- `workflow.rs`
- `.missiond/v2/*.lisp`

## 行为要求

Current behavior to preserve:

- no project signal: status `not_requested`, search core only.
- explicit absolute project path: resolved.
- project/target_project registry id: resolved.
- relative path: unresolved, no process cwd fallback.

New behavior:

- if `cwd` is an absolute path under a registered project path, resolve project root to the longest matching registered project root.
- response includes:
  - `project_root_status`
  - `project_root_source` value like `registry_longest_prefix`
  - `requested_cwd` when cwd was a subdir
  - diagnostic explaining match.

Do not break `flow_path` rule from last commit:

- relative `flow_path` still requires resolved project_root.

## Tests

Add focused tests in `flow_run.rs`:

- cwd at project root resolves.
- cwd under nested project root picks longest prefix.
- cwd under registered project subdir resolves root and records requested_cwd.
- relative cwd remains unresolved.
- relative flow_path still refuses without project_root.

## 验收

- `cargo test -p missiond-daemon handlers::compute::flow_run::tests`
- `cargo test -p missiond-daemon`
- `cargo build --workspace`
- `node scripts/check-architecture-lisp.mjs --all-v2`
- `git diff --check`

## 交付报告

说明：

- Resolver order.
- New response fields.
- Compatibility with generated flow loader.

