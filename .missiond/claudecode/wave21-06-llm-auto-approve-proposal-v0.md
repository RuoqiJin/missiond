# wave21-06-llm-auto-approve-proposal-v0 — LLM auto-approve proposal v0

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave21/wave21-06-llm-auto-approve-proposal-v0.lisp`

## Machine Contract

- kind: `code-alignment`
- status: `ready`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- depends_on: `wave20-08-review-auto-answer-policy-v0`
- shared_memory: `.missiond/tasks/wave21/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave21/reports/wave21-06-llm-auto-approve-proposal-v0.report.lisp`

## Goal

Add explicit Sonnet-assisted auto-approve proposal mode for directive/plan/review gates, without applying approvals automatically in v0.

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

1. Add explicit mode such as auto_approve_mode="sonnet_suggest"; default off.
2. LLM proposal must include decision, confidence, evidence, non_goal_check, destructive_check, and requires_human.
3. Never auto-reject and never auto-approve destructive/archive/supersede/remove actions.
4. If Sonnet unavailable, return LLM_UNAVAILABLE with no fallback.
5. Do not mutate directive/plan/review state in v0; response is proposal only.

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

Coordination ledger: `.missiond/tasks/wave21/shared-memory.lisp` (schema `missiond.shared-memory.v1`).

- Append a `claim` entry before starting work; append `observation` / `blocker` while running; append `completion` when done.
- Entries are append-only S-expressions; never edit prior entries — record fixes via a new `correction` entry.
- `:touched` paths in your entries must stay inside this task `:write-scope`.

Validate with:

```bash
node scripts/check-task-memory.mjs .missiond/tasks/wave21/shared-memory.lisp
```

## Report Contract

Expected machine-readable report: `.missiond/tasks/wave21/reports/wave21-06-llm-auto-approve-proposal-v0.report.lisp` (schema `missiond.report-contract.v1`).

- Required fields: `:schema`, `:task_id`, `:status`, `:commit_hash`, `:files_changed`, `:acceptance_results`.
- `:status` must be one of `draft | in-progress | done | blocked | rejected`; `done` requires non-empty `:acceptance_results`.
- Free-form prose belongs in `:notes`; structural fields drive automated verification.

Validate with:

```bash
node scripts/check-task-report.mjs .missiond/tasks/wave21/reports/wave21-06-llm-auto-approve-proposal-v0.report.lisp
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
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave21/wave21-06-llm-auto-approve-proposal-v0.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave21/wave21-06-llm-auto-approve-proposal-v0.lisp \
  git commit -m "feat(review): propose LLM auto approvals"
```

Scope check: `write-scope-only`.

The `task-scope-guard --mode staged` step blocks the commit before the index is locked in if any staged path falls outside `:write-scope` or matches `:must-not-touch`. The `MISSIOND_TASK_CONTRACT` env var activates the same check from the shared `.githooks/pre-commit` hook (enable per clone with `git config core.hooksPath .githooks`).

Verify the commit against this contract (read-only, post-commit):

```bash
node scripts/verify-task-contract.mjs .missiond/tasks/wave21/wave21-06-llm-auto-approve-proposal-v0.lisp
```

## Report

- `Commit hash.`
- `Mode name.`
- `Proposal schema.`
- `No-mutation proof.`
- `Acceptance command results.`

