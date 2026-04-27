# wave22-06-distill-chain-policy-auto-sonnet-v2 — Distill chain policy auto-Sonnet v2

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave22/wave22-06-distill-chain-policy-auto-sonnet-v2.lisp`

## Machine Contract

- kind: `code-alignment`
- status: `ready`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- depends_on: `wave21-07-sonnet-distill-chain-auto-apply-v1`
- shared_memory: `.missiond/tasks/wave22/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave22/reports/wave22-06-distill-chain-policy-auto-sonnet-v2.report.lisp`

## Goal

Replace the dual opt-in Sonnet distill chain gate with a single explicit policy gate that still requires all deterministic safety rules and review-required output.

## Ownership

- `crates/missiond-daemon/src/handlers/knowledge/workflow.rs`
- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`
- `crates/missiond-mcp/src/tools/knowledge/workflow.rs`

## Must Not Touch

- `crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs`
- `crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs`
- `crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs`
- `.missiond/v2/*.lisp`
- `scripts/**`

## Requirements

1. Add explicit auto_sonnet_policy such as off|safe_after_rules|dry_run; default off.
2. safe_after_rules must not require a second opt-in flag, but must require all deterministic safety rules to pass.
3. Every Sonnet-produced workflow/distill output must remain review_required=true; do not auto-approve workflow reuse.
4. If Sonnet unavailable or output invalid, return structured failure and preserve plan/workflow state.
5. Return policy_status, safety_rule_results, model_call_status, review_required, and sidecar path.

## Acceptance Commands

```bash
cargo test -p missiond-daemon handlers::knowledge::workflow::tests
cargo test -p missiond-daemon handlers::knowledge::plan::tests
cargo test -p missiond-daemon
cargo test -p missiond-mcp --lib
cargo build --workspace
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check -- crates/missiond-daemon/src/handlers/knowledge/workflow.rs crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-mcp/src/tools/knowledge/workflow.rs
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

Expected machine-readable report: `.missiond/tasks/wave22/reports/wave22-06-distill-chain-policy-auto-sonnet-v2.report.lisp` (schema `missiond.report-contract.v1`).

- Required fields: `:schema`, `:task_id`, `:status`, `:commit_hash`, `:files_changed`, `:acceptance_results`.
- `:status` must be one of `draft | in-progress | done | blocked | rejected`; `done` requires non-empty `:acceptance_results`.
- Free-form prose belongs in `:notes`; structural fields drive automated verification.

Validate with:

```bash
node scripts/check-task-report.mjs .missiond/tasks/wave22/reports/wave22-06-distill-chain-policy-auto-sonnet-v2.report.lisp
```

## Commit

After acceptance, commit only files inside the declared write scope.

Stage just the declared scope, run the pre-commit scoped-index guard, then commit:

```bash
git add "crates/missiond-daemon/src/handlers/knowledge/workflow.rs" \
        "crates/missiond-daemon/src/handlers/knowledge/plan.rs" \
        "crates/missiond-mcp/src/tools/knowledge/workflow.rs"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave22/wave22-06-distill-chain-policy-auto-sonnet-v2.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave22/wave22-06-distill-chain-policy-auto-sonnet-v2.lisp \
  git commit -m "feat(workflow): policy-gate automatic Sonnet distill"
```

Scope check: `write-scope-only`.

The `task-scope-guard --mode staged` step blocks the commit before the index is locked in if any staged path falls outside `:write-scope` or matches `:must-not-touch`. The `MISSIOND_TASK_CONTRACT` env var activates the same check from the shared `.githooks/pre-commit` hook (enable per clone with `git config core.hooksPath .githooks`).

Verify the commit against this contract (read-only, post-commit):

```bash
node scripts/verify-task-contract.mjs .missiond/tasks/wave22/wave22-06-distill-chain-policy-auto-sonnet-v2.lisp
```

## Report

- `Commit hash.`
- `Policy enum.`
- `Safety rule carryover.`
- `Review-required proof.`
- `Acceptance command results.`

