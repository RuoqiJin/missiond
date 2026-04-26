# Wave 18 Task 05 — Cross-Plan Distill Chain v0

## Goal

Implement a conservative cross-plan distill chain after successful plan finalization.

Wave17 can finalize a plan and optionally trigger distill. This task adds a reusable chain record so successful plans can feed workflow distillation across plan boundaries.

## Dependency

Run after Wave18-04 if plan execution files are being edited serially.

## Ownership

Expected files:

- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`
- `crates/missiond-daemon/src/handlers/knowledge/workflow.rs`
- `crates/missiond-daemon/src/handlers/knowledge/evidence_collector.rs` only if needed
- `crates/missiond-mcp/src/tools/knowledge/plan.rs`
- `crates/missiond-mcp/src/tools/knowledge/workflow.rs`

Avoid DB migrations unless an existing table cannot store the chain metadata.

## Requirements

1. Add optional inputs:

   - `distill_chain_id`
   - `distill_chain_mode`: `record_only | dry_run | sonnet`
   - `distill_chain_name`

2. Behavior:

   - only eligible when plan finalizes succeeded
   - record chain metadata in evidence sidecar or existing workflow metadata
   - `record_only`: no workflow distill call
   - `dry_run`: call workflow distill dry-run
   - `sonnet`: call workflow distill sonnet only when explicitly requested

3. If a chain already exists:

   - append new plan result
   - do not overwrite prior evidence

4. Failure:

   - distill chain failure surfaces warning/partial
   - successful plan finalization remains durable

5. Response:

   - `distill_chain_status`
   - `distill_chain_id`
   - `distill_result` or warning

## Tests

Add tests for:

- record_only writes chain evidence
- dry_run calls workflow dry-run path
- sonnet requires explicit mode
- failed plan skips chain
- chain append preserves prior entries

## Acceptance Commands

```bash
cargo test -p missiond-daemon handlers::knowledge::plan::tests
cargo test -p missiond-daemon handlers::knowledge::workflow::tests
cargo test -p missiond-daemon handlers::knowledge::evidence_collector::tests
cargo test -p missiond-daemon
cargo test -p missiond-mcp --lib
cargo build --workspace
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check
```

## Commit

```bash
git add crates/missiond-daemon/src/handlers/knowledge/plan.rs \
        crates/missiond-daemon/src/handlers/knowledge/workflow.rs \
        crates/missiond-daemon/src/handlers/knowledge/evidence_collector.rs \
        crates/missiond-mcp/src/tools/knowledge/plan.rs \
        crates/missiond-mcp/src/tools/knowledge/workflow.rs
git commit -m "feat(workflow): record cross-plan distill chains"
```

Only stage files actually modified.

## Report

Return:

- Commit hash.
- Chain metadata shape.
- Distill modes.
- Tests and acceptance results.
