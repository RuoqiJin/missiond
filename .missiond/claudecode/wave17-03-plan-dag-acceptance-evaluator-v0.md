# Wave 17 Task 03 — PLAN DAG Acceptance Evaluator v0

## Goal

Add a deterministic acceptance phase after node dispatch.

This task must not run arbitrary shell commands from PLAN.lisp. It should evaluate declared acceptance in a conservative way and surface a review/manual gate when acceptance cannot be proven.

## Dependency

Run after Wave17-02 if both are active, because both touch `plan_dag.rs`.

## Ownership

Expected files:

- `crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs`
- `crates/missiond-daemon/src/handlers/knowledge/evidence_collector.rs` only if a typed evidence helper is useful
- `crates/missiond-mcp/src/tools/knowledge/plan.rs`

Do not modify Lisp.

## Requirements

1. Parse node acceptance hints:

   - `:acceptance-commands [...]` already exists; keep it as declared commands, not executable commands.
   - optional `:acceptance-mode "inner_status" | "manual" | "evidence_keys"`
   - optional `:acceptance-evidence-keys [...]`

2. Default behavior:

   - If no acceptance hints exist, preserve current success/failure behavior.
   - If acceptance commands exist but no safe evaluator exists, mark `acceptance_status="manual_required"` and do not run shell.

3. Supported evaluators:

   - `inner_status`: accept when inner result has success/non-error status.
   - `evidence_keys`: accept when typed evidence contains all required keys.
   - `manual`: always pause or return manual_required.

4. Node lifecycle:

   - `dispatch succeeded` + acceptance accepted -> `succeeded`
   - `dispatch succeeded` + acceptance manual_required -> `paused` or `needs_acceptance`
   - `dispatch succeeded` + acceptance rejected -> `failed`

5. Evidence:

   - record acceptance mode
   - record declared commands without executing them
   - record acceptance status and reason

6. No shell execution.

## Tests

Add tests for:

- acceptance commands are surfaced but not executed
- inner_status accepts success result
- inner_status rejects error result
- evidence_keys accepts when keys exist
- evidence_keys rejects missing keys
- manual mode pauses/needs_acceptance

## Acceptance Commands

```bash
cargo test -p missiond-daemon handlers::knowledge::plan_dag::tests
cargo test -p missiond-daemon handlers::knowledge::evidence_collector::tests
cargo test -p missiond-daemon handlers::knowledge::plan::tests
cargo test -p missiond-daemon
cargo test -p missiond-mcp --lib
cargo build --workspace
node scripts/check-architecture-lisp.mjs --all-v2
git diff --check
```

## Commit

```bash
git add crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs \
        crates/missiond-daemon/src/handlers/knowledge/evidence_collector.rs \
        crates/missiond-mcp/src/tools/knowledge/plan.rs
git commit -m "feat(plan): evaluate DAG node acceptance safely"
```

Only stage files actually modified.

## Report

Return:

- Commit hash.
- Acceptance modes.
- Proof that shell commands are not executed.
- Evidence shape.
- Tests and acceptance results.
