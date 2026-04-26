# wave20-04-machine-driven-dispatch-v0 — Machine-driven task contract dispatch v0

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave20/wave20-04-machine-driven-dispatch-v0.lisp`

## Machine Contract

- kind: `code-alignment`
- status: `ready`
- owner: `claudecode`
- dispatch_strategy: `agent-team`
- depends_on: `wave20-02-renderer-scoped-commit-guard-v2`, `wave20-03-execution-preflight-contract-scope-v1`
- shared_memory: `.missiond/tasks/wave20/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave20/reports/wave20-04-machine-driven-dispatch-v0.report.lisp`

## Dispatch Note

使用 agent-team提高效率

## Goal

Add an internal dispatch mode where MissionD hands task.lisp directly to workstation_dispatch and treats Markdown as optional compatibility output, not the load-bearing contract.

## Ownership

- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`
- `crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs`
- `crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs`
- `crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs`
- `crates/missiond-mcp/src/tools/knowledge/plan.rs`

## Must Not Touch

- `crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs`
- `crates/missiond-daemon/src/handlers/knowledge/workflow.rs`
- `crates/missiond-core/src/event/events/execution.rs`
- `scripts/**`
- `.missiond/v2/*.lisp`

## Requirements

1. Use agent-team if useful: 使用 agent-team提高效率.
2. Add an opt-in mode such as dispatch_contract_mode="machine" or render_markdown=false; default remains current rendered brief behavior.
3. When machine mode is enabled and task_contract_path exists, workstation_dispatch must consume task.lisp directly and include task_contract_path in the returned descriptor.
4. Do not require Markdown generation for machine mode; if a rendered path exists, surface it as compatibility metadata only.
5. Malformed contract must return SafeDescriptor-style failure and must not fall back to claude -p or unscoped prompt mode.
6. Update unified_entry forwarding so the mode can be passed through the unified entry pipeline.

## Acceptance Commands

```bash
cargo test -p missiond-daemon handlers::knowledge::workstation_dispatch::tests
cargo test -p missiond-daemon handlers::knowledge::plan::tests
cargo test -p missiond-daemon handlers::knowledge::plan_dag::tests
cargo test -p missiond-daemon handlers::knowledge::unified_entry::tests
cargo test -p missiond-daemon
cargo test -p missiond-mcp --lib
cargo build --workspace
node scripts/check-task-contract.mjs --all
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check -- crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs crates/missiond-mcp/src/tools/knowledge/plan.rs
```

## Shared Memory

Coordination ledger: `.missiond/tasks/wave20/shared-memory.lisp` (schema `missiond.shared-memory.v1`).

- Append a `claim` entry before starting work; append `observation` / `blocker` while running; append `completion` when done.
- Entries are append-only S-expressions; never edit prior entries — record fixes via a new `correction` entry.
- `:touched` paths in your entries must stay inside this task `:write-scope`.

Validate with:

```bash
node scripts/check-task-memory.mjs .missiond/tasks/wave20/shared-memory.lisp
```

## Report Contract

Expected machine-readable report: `.missiond/tasks/wave20/reports/wave20-04-machine-driven-dispatch-v0.report.lisp` (schema `missiond.report-contract.v1`).

- Required fields: `:schema`, `:task_id`, `:status`, `:commit_hash`, `:files_changed`, `:acceptance_results`.
- `:status` must be one of `draft | in-progress | done | blocked | rejected`; `done` requires non-empty `:acceptance_results`.
- Free-form prose belongs in `:notes`; structural fields drive automated verification.

Validate with:

```bash
node scripts/check-task-report.mjs .missiond/tasks/wave20/reports/wave20-04-machine-driven-dispatch-v0.report.lisp
```

## Commit

After acceptance, commit only files inside the declared write scope.

```bash
git add "crates/missiond-daemon/src/handlers/knowledge/plan.rs" \
        "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs" \
        "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs" \
        "crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs" \
        "crates/missiond-mcp/src/tools/knowledge/plan.rs"
git commit -m "feat(plan): dispatch directly from Lisp task contracts"
```

Scope check: `write-scope-only`.

Verify the commit against this contract (read-only, post-commit):

```bash
node scripts/verify-task-contract.mjs .missiond/tasks/wave20/wave20-04-machine-driven-dispatch-v0.lisp
```

## Report

- `Commit hash.`
- `New mode/argument names.`
- `Machine-mode response fields.`
- `Fallback behavior.`
- `Acceptance command results.`

