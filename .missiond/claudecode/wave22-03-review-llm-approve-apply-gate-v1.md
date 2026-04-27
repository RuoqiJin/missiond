# wave22-03-review-llm-approve-apply-gate-v1 — Review LLM approval apply gate v1

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave22/wave22-03-review-llm-approve-apply-gate-v1.lisp`

## Machine Contract

- kind: `code-alignment`
- status: `ready`
- owner: `claudecode`
- dispatch_strategy: `agent-team`
- depends_on: `wave21-06-llm-auto-approve-proposal-v0`
- shared_memory: `.missiond/tasks/wave22/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave22/reports/wave22-03-review-llm-approve-apply-gate-v1.report.lisp`

## Dispatch Note

使用 agent-team提高效率

## Goal

Add an explicit apply gate for LLM approval proposals so non-destructive approvals can be applied only with caller approval, proposal hash matching, and deterministic safety checks.

## Ownership

- `crates/missiond-daemon/src/handlers/knowledge/review_gate.rs`
- `crates/missiond-daemon/src/handlers/knowledge/directive.rs`
- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`
- `crates/missiond-mcp/src/tools/knowledge/directive.rs`
- `crates/missiond-mcp/src/tools/knowledge/plan.rs`

## Must Not Touch

- `crates/missiond-daemon/src/handlers/knowledge/workflow.rs`
- `crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs`
- `crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs`
- `.missiond/v2/*.lisp`
- `scripts/**`

## Requirements

1. Use agent-team if useful: 使用 agent-team提高效率.
2. Add explicit apply_llm_auto_approve=true or equivalent; default remains proposal-only.
3. Require proposal_hash, caller_approved=true, non-destructive action, high confidence, and deterministic destructive_check=false before applying.
4. Never auto-reject. Never apply archive/supersede/remove/destructive actions.
5. On mismatch or missing proposal hash, return structured error and do not mutate directive/plan/review state.
6. Return apply_status, applied_decision, proposal_hash_status, and safety_rule_results.

## Acceptance Commands

```bash
cargo test -p missiond-daemon handlers::knowledge::review_gate::tests
cargo test -p missiond-daemon handlers::knowledge::directive::tests
cargo test -p missiond-daemon handlers::knowledge::plan::tests
cargo test -p missiond-daemon
cargo test -p missiond-mcp --lib
cargo build --workspace
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check -- crates/missiond-daemon/src/handlers/knowledge/review_gate.rs crates/missiond-daemon/src/handlers/knowledge/directive.rs crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-mcp/src/tools/knowledge/directive.rs crates/missiond-mcp/src/tools/knowledge/plan.rs
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

Expected machine-readable report: `.missiond/tasks/wave22/reports/wave22-03-review-llm-approve-apply-gate-v1.report.lisp` (schema `missiond.report-contract.v1`).

- Required fields: `:schema`, `:task_id`, `:status`, `:commit_hash`, `:files_changed`, `:acceptance_results`.
- `:status` must be one of `draft | in-progress | done | blocked | rejected`; `done` requires non-empty `:acceptance_results`.
- Free-form prose belongs in `:notes`; structural fields drive automated verification.

Validate with:

```bash
node scripts/check-task-report.mjs .missiond/tasks/wave22/reports/wave22-03-review-llm-approve-apply-gate-v1.report.lisp
```

## Commit

After acceptance, commit only files inside the declared write scope.

Stage just the declared scope, run the pre-commit scoped-index guard, then commit:

```bash
git add "crates/missiond-daemon/src/handlers/knowledge/review_gate.rs" \
        "crates/missiond-daemon/src/handlers/knowledge/directive.rs" \
        "crates/missiond-daemon/src/handlers/knowledge/plan.rs" \
        "crates/missiond-mcp/src/tools/knowledge/directive.rs" \
        "crates/missiond-mcp/src/tools/knowledge/plan.rs"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave22/wave22-03-review-llm-approve-apply-gate-v1.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave22/wave22-03-review-llm-approve-apply-gate-v1.lisp \
  git commit -m "feat(review): gate applying LLM approval proposals"
```

Scope check: `write-scope-only`.

The `task-scope-guard --mode staged` step blocks the commit before the index is locked in if any staged path falls outside `:write-scope` or matches `:must-not-touch`. The `MISSIOND_TASK_CONTRACT` env var activates the same check from the shared `.githooks/pre-commit` hook (enable per clone with `git config core.hooksPath .githooks`).

Verify the commit against this contract (read-only, post-commit):

```bash
node scripts/verify-task-contract.mjs .missiond/tasks/wave22/wave22-03-review-llm-approve-apply-gate-v1.lisp
```

## Report

- `Commit hash.`
- `Apply gate fields.`
- `Safety rule matrix.`
- `No auto-reject/destructive proof.`
- `Acceptance command results.`

