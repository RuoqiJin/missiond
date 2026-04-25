# MissionD v2 Code Alignment Task: Global Instruction Manager

请按 MissionD v2 Lisp 架构做代码同构：实现 `mission_global_instruction` MCP surface，用于读取、编辑、reload 全局 `~/.claude/CLAUDE.md`。

只做代码同构，不重新设计架构，不修改 `.missiond/v2/*.lisp`。当前 Lisp 是工作树里的最新设计，请先读这些锚点：

- `/Users/jinchen/Projects/missiond/.missiond/v2/intent-intent-layer.lisp` :: `global-claudemd-manager`
- `/Users/jinchen/Projects/missiond/.missiond/v2/intent-tools.lisp` :: `mission_global_instruction`
- `/Users/jinchen/Projects/missiond/.missiond/v2/intent-flow.lisp` :: future-flow mapping for `mission_global_instruction`

## Parallel Scope

This is a small MCP surface task, but it touches registration files:

- `crates/missiond-mcp/src/tools/**`
- `crates/missiond-daemon/src/handlers/**`
- `crates/missiond-daemon/src/handlers/mod.rs`
- `crates/missiond-mcp/src/tools/mod.rs`

Do not run this simultaneously with the directive/plan/workflow surface task unless you are ready to merge MCP registration conflicts.

## Actions

Implement `mission_global_instruction` actions:

1. `read`
   - read `~/.claude/CLAUDE.md`
   - return content, path, size, mtime/hash if easy
2. `edit`
   - accept either full `new_content` or a small structured patch if the codebase already has patch helpers
   - support `dry_run`
   - write via temp file + rename
   - create timestamped backup before overwriting
3. `reload`
   - if daemon/agent reload hook exists, call it
   - otherwise return `manual-reload-required` with a clear status; do not pretend reload happened

## Safety Contract

- Never edit arbitrary paths; only `~/.claude/CLAUDE.md`.
- Reject empty destructive writes unless `allow_empty=true`.
- Preserve UTF-8.
- Run any existing architecture/style checker only if the changed content contains structured MissionD blocks; otherwise do not overreach.
- `dry_run=true` must not write.

## Non-Goals

- Do not modify `.missiond/v2/*.lisp`.
- Do not implement project-local CLAUDE.md management in this task.
- Do not add a new database table.

## Acceptance

- `cargo build --workspace`
- `cargo test -p missiond-daemon`
- `cargo test -p missiond-mcp`
- `node scripts/check-architecture-lisp.mjs --all-v2`
- `git diff --check`

Add focused tests where practical:

- read missing file returns structured not-found or empty state
- dry-run edit returns diff/preview and does not write
- edit writes backup + atomically replaces file
- reload returns supported/manual status honestly

## Deliverables

- List modified files.
- Mark each action status: `full` / `dry-run` / `manual` / `not-implemented`.
- State backup path convention.
