# wave22-05-autonomous-workstation-true-spawn-v1 — Autonomous workstation true spawn v1

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave22/wave22-05-autonomous-workstation-true-spawn-v1.lisp`

## Machine Contract

- kind: `code-alignment`
- status: `ready`
- owner: `claudecode`
- dispatch_strategy: `agent-team`
- depends_on: `wave21-04-autonomous-workstation-llm-proposal-v0`, `wave22-02-execution-auto-run-verifier-v2`
- shared_memory: `.missiond/tasks/wave22/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave22/reports/wave22-05-autonomous-workstation-true-spawn-v1.report.lisp`

## Dispatch Note

使用 agent-team提高效率

## Goal

Promote autonomous workstation proposals to real mission_task_delegate dispatch only under explicit auto_spawn gate and verified task-contract scope.

## Ownership

- `crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs`
- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`
- `crates/missiond-mcp/src/tools/knowledge/plan.rs`

## Must Not Touch

- `crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs`
- `crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs`
- `crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs`
- `.missiond/v2/*.lisp`
- `scripts/**`

## Requirements

1. Use agent-team if useful: 使用 agent-team提高效率.
2. Add explicit auto_spawn=true gate; default remains proposal-only.
3. Require proposal_hash match, high confidence, validated task_contract_path, non-empty write-scope, hooks/preflight status acceptable, and no forbidden scope overlap.
4. Dispatch only through mission_task_delegate / workstation substrate; never claude -p.
5. If any gate fails, return SafeDescriptor-style structured failure and do not spawn.
6. Return auto_spawn_status, spawn_target, task_contract_path, proposal_hash_status, and gate_results.

## Acceptance Commands

```bash
cargo test -p missiond-daemon handlers::knowledge::workstation_dispatch::tests
cargo test -p missiond-daemon handlers::knowledge::plan::tests
cargo test -p missiond-daemon
cargo test -p missiond-mcp --lib
cargo build --workspace
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check -- crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-mcp/src/tools/knowledge/plan.rs
```

## Shared Memory

Coordination ledger: `.missiond/tasks/wave22/shared-memory.lisp` (schema `missiond.shared-memory.v1`).

- Append a `claim` entry before starting work; append `observation` / `blocker` while running; append `completion` when done.
- Entries are append-only S-expressions; never edit prior entries — record fixes via a new `correction` entry.
- `:touched` paths in your entries must stay inside this task `:write-scope`.

Validate with:

```bash
node scripts/check-task-memory.mjs .missiond/tasks/wave22/shared-memory.lisp
```

## Report Contract

Expected machine-readable report: `.missiond/tasks/wave22/reports/wave22-05-autonomous-workstation-true-spawn-v1.report.lisp` (schema `missiond.report-contract.v1`).

- Required fields: `:schema`, `:task_id`, `:status`, `:commit_hash`, `:files_changed`, `:acceptance_results`.
- `:status` must be one of `draft | in-progress | done | blocked | rejected`; `done` requires non-empty `:acceptance_results`.
- Free-form prose belongs in `:notes`; structural fields drive automated verification.

Validate with:

```bash
node scripts/check-task-report.mjs .missiond/tasks/wave22/reports/wave22-05-autonomous-workstation-true-spawn-v1.report.lisp
```

## Commit

After acceptance, commit only files inside the declared write scope.

Stage just the declared scope, run the pre-commit scoped-index guard, then commit:

```bash
git add "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs" \
        "crates/missiond-daemon/src/handlers/knowledge/plan.rs" \
        "crates/missiond-mcp/src/tools/knowledge/plan.rs"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave22/wave22-05-autonomous-workstation-true-spawn-v1.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave22/wave22-05-autonomous-workstation-true-spawn-v1.lisp \
  git commit -m "feat(workstation): gate autonomous task dispatch"
```

Scope check: `write-scope-only`.

The `task-scope-guard --mode staged` step blocks the commit before the index is locked in if any staged path falls outside `:write-scope` or matches `:must-not-touch`. The `MISSIOND_TASK_CONTRACT` env var activates the same check from the shared `.githooks/pre-commit` hook (enable per clone with `git config core.hooksPath .githooks`).

Verify the commit against this contract (read-only, post-commit):

```bash
node scripts/verify-task-contract.mjs .missiond/tasks/wave22/wave22-05-autonomous-workstation-true-spawn-v1.lisp
```

## Report

- `Commit hash.`
- `auto_spawn gate matrix.`
- `Substrate path proof.`
- `No claude -p fallback proof.`
- `Acceptance command results.`

