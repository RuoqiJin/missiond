# MissionD v2 Code Alignment Task: Project-Root Spawn CWD Contract

请按 MissionD v2 Lisp 架构做下一批代码同构：实现 CLI 工位 spawn 的 project-root cwd 契约。

只做代码同构，不重新设计架构，不修改 `.missiond/v2/*.lisp`。当前 `.missiond/v2` 的 Lisp 是工作树里的最新设计，请先读这些锚点：

- `/Users/jinchen/Projects/missiond/.missiond/v2/intent-worker.lisp` :: `project-root-spawn-cwd`
- `/Users/jinchen/Projects/missiond/.missiond/v2/intent-flow.lisp` :: `F-workflow-slot-full-lifecycle` :: `s1b-target-project-root`
- `/Users/jinchen/Projects/missiond/.missiond/v2/intent-flow.lisp` :: `F-dynamic-slot-lifecycle`
- `/Users/jinchen/Projects/missiond/.missiond/v2/intent-flow.lisp` :: `F-task-delegate-autoprovision`
- `/Users/jinchen/Projects/missiond/.missiond/v2/intent-tools.lisp` :: `mission_pty_spawn` / `mission_task_delegate` / `mission_agent` / `mission_compute_slot`

## Contract

For every project-bound ClaudeCode / Gemini CLI / Codex CLI slot spawn:

1. Resolve `target_project_root` before spawn or slot reuse.
2. Process cwd must equal `target_project_root`.
3. Caller-supplied `cwd` below a project root is context metadata only. Do not use it as process cwd after root resolution.
4. Existing slot reuse is allowed only when `slot.project_root == target_project_root`.
5. If the project root cannot be resolved, fail fast with a structured error. Do not silently use daemon cwd, home, tmp, or the requested subdir.

Resolution order:

1. explicit `project_id` / `projectId` -> registered project root
2. explicit `cwd` -> `ProjectRegistry::resolve(cwd)` longest-prefix -> registered project root
3. board task / dynamic slot config project id -> registered project root
4. slot config default project root only for registered project-bound slots

## Goals

1. Add or reuse a small resolver helper for `target_project_root` near the slot/spawn boundary. Prefer a single helper used by callers instead of duplicating partial logic.
2. Update `mission_pty_spawn`, `mission_agent` spawn/restart, `mission_compute_slot(action=create)`, and `mission_task_delegate` auto-provision/reuse paths to resolve and pass project root.
3. Update slot config/runtime metadata so project-bound slots retain:
   - resolved `project_root`
   - optional `requested_cwd` for prompt/context/audit only
4. Update `spawn_tracked_slot` or the immediate caller boundary so it asserts process cwd == project root for project-bound CLI engines.
5. For Gemini/Codex CLI, hard fail on unresolved root. For ClaudeCode, also use project root because project memory, JSONL encoded path, permissions, and tool paths are more stable there.
6. Update learned permission injection so project-scope settings write to `<project_root>/.claude/settings.local.json`, not a subdir cwd.
7. Preserve existing behavior for non-project-bound/non-CLI internal process flows unless they naturally share the same slot config.

## Known Code Anchors To Inspect

- `crates/missiond-core/src/types/project.rs` — `ProjectRegistry::resolve`
- `crates/missiond-core/src/types/slot.rs` — current `SlotConfig`
- `crates/missiond-daemon/src/slot_orchestrator/spawner.rs` — `spawn_tracked_slot`
- `crates/missiond-daemon/src/slot_orchestrator/types.rs`
- `crates/missiond-daemon/src/slot_orchestrator/perm_injector.rs`
- `crates/missiond-daemon/src/handlers/compute/pty.rs`
- `crates/missiond-daemon/src/handlers/compute/process.rs`
- `crates/missiond-daemon/src/handlers/compute/compute_slot.rs`
- `crates/missiond-daemon/src/handlers/compute/task_delegate.rs`
- `crates/missiond-daemon/src/handlers/compute/task.rs`
- `crates/missiond-daemon/src/main.rs` reload/autostart slot paths

## Non-Goals

- Do not redesign memory/system-layer Lisp.
- Do not add a new memory table unless strictly necessary.
- Do not implement unrelated slot UI behavior.
- Do not stage or commit `.missiond/v2/*.lisp`.
- Do not use requested subdir as process cwd for project-bound CLI slot spawn.

## Acceptance

- `cargo build --workspace`
- relevant `cargo test`:
  - `cargo test -p missiond-core --lib`
  - `cargo test -p missiond-daemon`
  - `cargo test -p missiond-mcp`
- `node scripts/check-architecture-lisp.mjs --all-v2`
- `git diff --check`

Add focused unit tests where practical:

- cwd inside a registered project resolves to that project root
- unresolved cwd fails for project-bound CLI spawn
- slot reuse rejects mismatched `project_root`
- `requested_cwd` is preserved as metadata but process cwd is project root

## Deliverables

- List modified files.
- State which spawn/reuse paths are full / partial / unchanged and why.
- State whether any existing behavior is intentionally preserved.
- If a path is only dry-run/read-only/partial, explain the blocker clearly.
