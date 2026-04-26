# Wave 18 Task 09 — Unified Entry Autonomous Loop Smoke v1

## Goal

Add a deterministic smoke test for the most complete current MissionD loop.

This should cover the post-Wave17/Wave18 backend loop without frontend and without external ClaudeCode/LLM.

## Dependency

Run after the Wave18 tasks that land.

## Ownership

Expected files:

- `crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs`
- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`
- `crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs`
- `crates/missiond-daemon/src/handlers/knowledge/workflow.rs`
- `crates/missiond-daemon/src/handlers/knowledge/evidence_collector.rs`

Do not modify Lisp.

## Scenario

Build a deterministic smoke that exercises as many landed pieces as possible:

1. directive/plan fixture or dry-run compile
2. PLAN DAG execute with:
   - claim/lease
   - workstation dispatch dry-run or safe descriptor
   - acceptance evaluator
   - paused review + resume if available
   - retry if cheap to simulate
   - finalize
   - distill chain dry-run/record-only if available
3. evidence sidecar written/read
4. no LLM
5. no external ClaudeCode spawn
6. no shell command execution from PLAN acceptance

## Requirements

1. Assert stable response fields:

   - pipeline stage
   - aggregate status
   - evidence path
   - finalization status
   - distill chain status if available

2. Assert v0 non-goals remain surfaced where relevant.

3. Keep the smoke focused. Do not duplicate every unit branch.

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
        crates/missiond-daemon/src/handlers/knowledge/workflow.rs \
        crates/missiond-daemon/src/handlers/knowledge/evidence_collector.rs
git commit -m "test(intent): cover autonomous loop smoke"
```

Only stage files actually modified.

## Report

Return:

- Commit hash.
- Smoke scenario covered.
- Which Wave18 tasks were included.
- Tests and acceptance results.
