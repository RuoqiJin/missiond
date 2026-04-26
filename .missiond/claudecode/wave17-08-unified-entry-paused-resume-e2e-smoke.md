# Wave 17 Task 08 — Unified Entry Paused-Resume E2E Smoke

## Goal

Add deterministic smoke coverage for the review-pause/resume path.

Wave16 added e2e smoke for the happy-path 4 hand-off. Wave17 should prove that a PLAN DAG node can pause for review, consume a deterministic review resolution, and resume/finalize without live LLM or external ClaudeCode.

## Dependency

Run after the Wave17 tasks that land:

- Wave17-01 paused resume hook
- Wave17-03 acceptance evaluator, if completed
- Wave17-05 finalize/distill trigger, if completed

## Ownership

Expected files:

- `crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs`
- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`
- `crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs`
- `crates/missiond-daemon/src/handlers/knowledge/review_gate.rs` only for test helper reuse

Do not modify Lisp.

## Requirements

1. Deterministic test only:

   - no Sonnet/Gemini
   - no external ClaudeCode spawn
   - no shell command execution

2. Scenario:

   - create/prepare plan fixture with one node requiring `:review-gate "question-event"`
   - execute -> node pauses, question id surfaced
   - resolve approved -> resume node
   - node reaches terminal state
   - evidence sidecar records pause and resume

3. If finalization task landed:

   - assert final plan status behavior.

4. If acceptance evaluator landed:

   - include an `inner_status` acceptance check.

5. The test should not duplicate every unit branch. It is a smoke contract.

## Acceptance Commands

```bash
cargo test -p missiond-daemon handlers::knowledge::unified_entry::tests
cargo test -p missiond-daemon handlers::knowledge::plan::tests
cargo test -p missiond-daemon handlers::knowledge::plan_dag::tests
cargo test -p missiond-daemon
cargo test -p missiond-mcp --lib
cargo build --workspace
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check
```

## Commit

```bash
git add crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs \
        crates/missiond-daemon/src/handlers/knowledge/plan.rs \
        crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs \
        crates/missiond-daemon/src/handlers/knowledge/review_gate.rs
git commit -m "test(intent): cover paused DAG resume smoke"
```

Only stage files actually modified.

## Report

Return:

- Commit hash.
- Smoke scenario.
- Features included/excluded based on landed tasks.
- Tests and acceptance results.
