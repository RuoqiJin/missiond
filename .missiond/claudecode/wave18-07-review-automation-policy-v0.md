# Wave 18 Task 07 — Review Automation Policy v0

## Goal

Add an explicit policy layer for review automation without turning on unsafe auto-approval.

Wave16/17 can create, resolve, and resume review gates. This task adds policy plumbing for future auto approve / auto answer while keeping default behavior manual.

## Ownership

Expected files:

- `crates/missiond-daemon/src/handlers/knowledge/review_gate.rs`
- `crates/missiond-daemon/src/handlers/knowledge/directive.rs`
- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`
- `crates/missiond-daemon/src/handlers/knowledge/workflow.rs`
- `crates/missiond-mcp/src/tools/knowledge/directive.rs`
- `crates/missiond-mcp/src/tools/knowledge/plan.rs`
- `crates/missiond-mcp/src/tools/knowledge/workflow.rs`

Do not add a new MCP tool.

## Requirements

1. Add optional policy input:

   - `review_automation_policy`: `manual | suggest | auto_safe`
   - default `manual`

2. Behavior:

   - `manual`: current behavior
   - `suggest`: return suggested decision, no mutation
   - `auto_safe`: only auto-resolve when deterministic rules prove safety

3. Deterministic safety rules for `auto_safe`:

   - artifact produced by deterministic/dry-run mode
   - no file write or file hash matches expected
   - no protected source/target
   - no unresolved conflicts
   - caller explicitly opted in

4. No live LLM auto-approval in this task.

5. Response:

   - `review_automation_policy`
   - `review_automation_status`
   - `suggested_review_decision`
   - `automation_reasons`

6. Existing explicit review resolution remains authoritative.

## Tests

Add tests for:

- default manual unchanged
- suggest returns decision without mutation
- auto_safe approves only deterministic safe artifact
- auto_safe refuses protected/conflicted artifact
- explicit human decision overrides suggestion

## Acceptance Commands

```bash
cargo test -p missiond-daemon handlers::knowledge::review_gate::tests
cargo test -p missiond-daemon handlers::knowledge::directive::tests
cargo test -p missiond-daemon handlers::knowledge::plan::tests
cargo test -p missiond-daemon handlers::knowledge::workflow::tests
cargo test -p missiond-daemon
cargo test -p missiond-mcp --lib
cargo build --workspace
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check
```

## Commit

```bash
git add crates/missiond-daemon/src/handlers/knowledge/review_gate.rs \
        crates/missiond-daemon/src/handlers/knowledge/directive.rs \
        crates/missiond-daemon/src/handlers/knowledge/plan.rs \
        crates/missiond-daemon/src/handlers/knowledge/workflow.rs \
        crates/missiond-mcp/src/tools/knowledge/directive.rs \
        crates/missiond-mcp/src/tools/knowledge/plan.rs \
        crates/missiond-mcp/src/tools/knowledge/workflow.rs
git commit -m "feat(review): add explicit automation policy"
```

Only stage files actually modified.

## Report

Return:

- Commit hash.
- Policy contract.
- Safety rules.
- Tests and acceptance results.
