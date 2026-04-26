# Wave 15 Task 01 — Fix Event Dispatcher Domain Count Integration Test

## Goal

Fix the remaining stale hardcoded "12 domains" integration test after `Domain::Execution` made the real domain count 13.

This is a small correction task. The test must assert the extensible contract instead of freezing the historical count.

## Known Failure

Current failing command:

```bash
cargo test -p missiond-core --test event_dispatcher_integration
```

Current failure:

```text
test domain_all_length_is_12 ... FAILED
left: 13
right: 12
```

## Ownership

Primary ownership:

- `crates/missiond-core/tests/event_dispatcher_integration.rs`

Optional wording-only ownership, only if needed for stale nearby documentation:

- `.missiond/v2/intent-event-bus.lisp`

Do not modify daemon, MCP, DB migrations, generated flow code, plan DAG code, or unrelated Lisp files.

## Requirements

1. Replace the hardcoded `assert_eq!(Domain::ALL.len(), 12)` style check with an extensible assertion.

   Preferred shape:

   - Rename the test from `domain_all_length_is_12` to something like `domain_all_includes_execution`.
   - Assert that `Domain::ALL` contains `Domain::Execution`.
   - Assert `Domain::ALL.len() >= 13` only if you need a regression guard.
   - Do not assert an exact domain count unless the assertion is derived from a single source of truth.

2. Update stale test comments such as "register all 12 domains" to "register all current domains" or equivalent.

3. If you touch `.missiond/v2/intent-event-bus.lisp`, keep it wording-only and preserve all existing section anchors.

4. Do not change `Domain::ALL`, dispatcher registration, event variants, or production code.

## Acceptance Commands

Run:

```bash
cargo test -p missiond-core --test event_dispatcher_integration
cargo test -p missiond-core --lib
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check
```

Expected:

- `event_dispatcher_integration` passes.
- Core lib tests pass.
- Lisp checker passes if any Lisp wording was touched.
- Whitespace check is clean.

## Commit

After acceptance, commit only your owned files:

```bash
git add crates/missiond-core/tests/event_dispatcher_integration.rs
git add .missiond/v2/intent-event-bus.lisp # only if actually modified
git commit -m "test(event): stop hardcoding domain count"
```

## Report

Return:

- Commit hash.
- Files changed.
- Exact new assertion strategy.
- Acceptance command results.
