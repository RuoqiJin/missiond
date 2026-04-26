# wave20-08-review-auto-answer-policy-v0 — Review auto-answer policy v0

> Generated from MissionD task-contract v1.
> Source: `.missiond/tasks/wave20/wave20-08-review-auto-answer-policy-v0.lisp`

## Machine Contract

- kind: `code-alignment`
- status: `ready`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- shared_memory: `.missiond/tasks/wave20/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave20/reports/wave20-08-review-auto-answer-policy-v0.report.lisp`

## Goal

Implement a safe, explicit auto-answer policy for review questions where deterministic safety rules can answer non-destructive questions without human intervention.

## Ownership

- `crates/missiond-daemon/src/handlers/knowledge/review_gate.rs`
- `crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs`

## Must Not Touch

- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`
- `crates/missiond-daemon/src/handlers/knowledge/workflow.rs`
- `crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs`
- `crates/missiond-core/src/event/events/execution.rs`
- `.missiond/v2/*.lisp`
- `scripts/**`

## Requirements

1. Add explicit auto_answer_policy with modes off|deterministic_safe|dry_run; default off.
2. Never auto-reject and never auto-approve destructive/archive/supersede/remove actions.
3. Return policy_result, selected_decision, safety_rule_results, and requires_human when skipped.
4. Wire unified_entry to surface the policy in dry-run smoke paths without adding a new MCP tool.

## Acceptance Commands

```bash
cargo test -p missiond-daemon handlers::knowledge::review_gate::tests
cargo test -p missiond-daemon handlers::knowledge::unified_entry::tests
cargo test -p missiond-daemon
cargo build --workspace
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check -- crates/missiond-daemon/src/handlers/knowledge/review_gate.rs crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs
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

Expected machine-readable report: `.missiond/tasks/wave20/reports/wave20-08-review-auto-answer-policy-v0.report.lisp` (schema `missiond.report-contract.v1`).

- Required fields: `:schema`, `:task_id`, `:status`, `:commit_hash`, `:files_changed`, `:acceptance_results`.
- `:status` must be one of `draft | in-progress | done | blocked | rejected`; `done` requires non-empty `:acceptance_results`.
- Free-form prose belongs in `:notes`; structural fields drive automated verification.

Validate with:

```bash
node scripts/check-task-report.mjs .missiond/tasks/wave20/reports/wave20-08-review-auto-answer-policy-v0.report.lisp
```

## Commit

After acceptance, commit only files inside the declared write scope.

```bash
git add "crates/missiond-daemon/src/handlers/knowledge/review_gate.rs" \
        "crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"
git commit -m "feat(review): add deterministic auto-answer policy"
```

Scope check: `write-scope-only`.

Verify the commit against this contract (read-only, post-commit):

```bash
node scripts/verify-task-contract.mjs .missiond/tasks/wave20/wave20-08-review-auto-answer-policy-v0.lisp
```

## Report

- `Commit hash.`
- `Policy modes.`
- `Safety rules.`
- `Non-destructive boundaries.`
- `Acceptance command results.`

