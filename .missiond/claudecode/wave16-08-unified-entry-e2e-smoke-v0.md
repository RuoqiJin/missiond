# Wave 16 Task 08 — Unified Entry E2E Smoke v0

## Goal

Add a focused smoke test for the canonical MissionD loop:

```text
message/directive input -> directive draft -> plan draft/approve -> execute -> evidence
```

This should be a deterministic local test, not a live LLM test and not an external ClaudeCode spawn.

## Dependency

Run after Wave16-01 through Wave16-07 tasks that land in this wave, or explicitly limit the smoke to already-committed features.

## Ownership

Expected files:

- `crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs`
- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`
- `crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs` only if the smoke uses scheduler mode

Optional test support files only if existing patterns require them.

Do not modify Lisp in this task.

## Requirements

1. Add a deterministic smoke test that does not call Sonnet/Gemini/ClaudeCode.

2. Use dry-run/deterministic modes where needed:

   - directive compile dry-run or persisted draft with local sexp
   - plan compile dry-run or prebuilt approved plan fixture
   - execute dry-run or internal target that is safe in test
   - evidence sidecar written to temp project root if needed

3. Assert response pipeline fields:

   - current stage
   - next step
   - artifact refs
   - evidence path/ref or dry-run would_dispatch

4. If review gates are included, use explicit resolution helpers; do not require live bus.

5. The test should prove integration shape, not every branch.

## Acceptance Commands

```bash
cargo test -p missiond-daemon handlers::knowledge::unified_entry::tests
cargo test -p missiond-daemon handlers::knowledge::plan::tests
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
        crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs
git commit -m "test(intent): add unified entry smoke coverage"
```

Only stage files actually modified.

## Report

Return:

- Commit hash.
- Smoke scenario covered.
- What remains outside smoke coverage.
- Tests and acceptance results.
