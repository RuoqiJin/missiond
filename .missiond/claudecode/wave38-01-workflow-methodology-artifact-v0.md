# wave38-01-workflow-methodology-artifact-v0 — compile_methodology writes V3 workflow artifact v0

> Thin brief rendered from MissionD task-contract v1. Task Lisp remains the SSOT.
> Source: `.missiond/tasks/wave38/wave38-01-workflow-methodology-artifact-v0.lisp`
> Shared preamble: `.missiond/claudecode/wave38-shared-preamble.md`

## Task Contract

- kind: `code-alignment`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- verification_tier: `local`
- dispatch_group: `A`
- estimated_minutes: `50`
- heartbeat_minutes: `10`
- shared_memory: `.missiond/tasks/wave38/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave38/reports/wave38-01-workflow-methodology-artifact-v0.report.lisp`
- session_trace: `.missiond/tasks/wave38/session-trace.lisp` (writable)
- router_policy: `.missiond/router/router-policy-v1.lisp` (advisory / dry-run only)
- router_backend_registry: `.missiond/router/router-backend-registry-v1.lisp` (MUST NOT switch backend)
- context_atlas: `.missiond/tasks/wave38/context-atlas.lisp`
- pattern_card: `.missiond/tasks/wave38/pattern-cards.lisp`

## Context Navigation

- Read context atlas first: `.missiond/tasks/wave38/context-atlas.lisp`.
- Follow implementation pattern card: `.missiond/tasks/wave38/pattern-cards.lisp`.
- Use atlas grep anchors and pattern-card conventions before falling back to broad scans.

## Goal

Close the workflow Lisp-isomorphism gap left in V3: mission_workflow action=compile_methodology already reads methodology Lisp and persists generated YAML, but write_file currently canonicalizes methodology source because that branch has no workflow row. Make write_file project the methodology compile into an enriched V3 workflow artifact under .missiond/workflows/<topic>.lisp, so the file remains the reviewable Lisp truth rather than a raw source snapshot.

## Ownership

- `.missiond/v3/missiond-blueprint.lisp`
- `scripts/check-v3-workflow-isomorphism.mjs`
- `crates/missiond-daemon/src/handlers/knowledge/workflow.rs`
- `crates/missiond-mcp/src/tools/knowledge/workflow.rs`

## Must Not Touch

- `packages/**`
- `crates/missiond-daemon/src/handlers/knowledge/request.rs`
- `crates/missiond-daemon/src/handlers/knowledge/plan.rs`
- `crates/missiond-daemon/src/handlers/knowledge/directive.rs`
- `crates/missiond-daemon/src/handlers/compute/**`
- `scripts/check-verification-receipt.mjs`
- `scripts/check-v3-task-lifecycle-isomorphism.mjs`
- `scripts/verify-task-runner-batch.mjs`
- `.missiond/v1/**`
- `.missiond/v2/**`
- `.missiond/research/**`
- `.missiond/tasks/schema/**`
- `.missiond/tasks/wave29/**`
- `.missiond/tasks/wave30/**`
- `.missiond/tasks/wave31/**`
- `.missiond/tasks/wave32/**`
- `.missiond/tasks/wave33/**`
- `.missiond/tasks/wave34/**`
- `.missiond/tasks/wave35/**`
- `.missiond/tasks/wave36/**`
- `.missiond/tasks/wave37/**`
- `.missiond/tasks/wave38/manifest.lisp`
- `.missiond/tasks/wave38/context-atlas.lisp`
- `.missiond/tasks/wave38/pattern-cards.lisp`
- `.missiond/tasks/wave38/wave38-*.lisp`
- `.missiond/claudecode/**`

## Requirements

1. Update .missiond/v3/missiond-blueprint.lisp first. The mission_workflow surface should no longer say compile_methodology write_file canonicalizes methodology source because no workflow_id row. It should state the V3 artifact projection rule for methodology compiles.
2. For action=compile_methodology compile_mode=deterministic persist=true write_file=true, write .missiond/workflows/<topic>.lisp as an enriched V3 workflow artifact, not the raw/canonicalized methodology source. The artifact should include stable refs such as :workflow_id or methodology flow id, :source_plans [], :match_rules metadata for source_kind/compiler/source_hash/flow_id, :steps, :status, and :body containing the methodology Lisp body.
3. Keep existing distill persist+write_file behavior intact. The distill path should still write enriched V3 workflow artifacts backed by real workflow_id rows.
4. Keep compile_methodology YAML generation, run_methodology, review gate policy, and no-DB-row semantics intact. Do not introduce a workflow DB migration for this task.
5. Update scripts/check-v3-workflow-isomorphism.mjs to pin the new contract text and code-level helper/fixture names.
6. Add focused Rust unit coverage that demonstrates compile_methodology methodology compile artifacts are rendered as V3 workflow artifacts and include the generated flow id/source hash metadata rather than a raw source-only file.
7. Update the MCP workflow tool description so callers know compile_methodology write_file writes the enriched V3 workflow artifact, while the methodology branch still has no Workflow row DB transition.
8. Preserve backward-compatible defaults: write_file=false remains preview/YAML only; dry_run remains a lint preview; no action should dispatch workstation work.

## Acceptance Commands

```bash
node scripts/check-v3-workflow-isomorphism.mjs --dry-fixture
node scripts/check-v3-workflow-isomorphism.mjs
cargo test -p missiond-daemon handlers::knowledge::workflow::tests:: --quiet
cargo test -p missiond-mcp test_directive_plan_workflow_surfaces_registered --quiet
node scripts/check-lisp-blueprint-compression.mjs
node scripts/check-architecture-lisp.mjs --no-structure .missiond/v3/missiond-blueprint.lisp
perl -ne 'exit 1 if /\x00/' .missiond/v3/missiond-blueprint.lisp scripts/check-v3-workflow-isomorphism.mjs crates/missiond-daemon/src/handlers/knowledge/workflow.rs crates/missiond-mcp/src/tools/knowledge/workflow.rs
git diff --check -- .missiond/v3/missiond-blueprint.lisp scripts/check-v3-workflow-isomorphism.mjs crates/missiond-daemon/src/handlers/knowledge/workflow.rs crates/missiond-mcp/src/tools/knowledge/workflow.rs
```

## Shared Protocol

Read `.missiond/claudecode/wave38-shared-preamble.md` once for shared-memory, report, session-trace, router, hook, commit, and verifier protocol. Do not paste or duplicate that boilerplate into this task.
- Task-specific scope and acceptance above override generic guidance.
- Load the context atlas / pattern card before broad repository search; use their anchors to reduce navigation misses.
- Append coordination facts to shared memory when present; write the report contract when the task completes.
- If work is still active after 10 minutes without a completion, append a heartbeat/observation entry or report a blocker.

## Commit

Commit only files inside the declared write scope after acceptance:

```bash
git add ".missiond/v3/missiond-blueprint.lisp" \
        "scripts/check-v3-workflow-isomorphism.mjs" \
        "crates/missiond-daemon/src/handlers/knowledge/workflow.rs" \
        "crates/missiond-mcp/src/tools/knowledge/workflow.rs"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave38/wave38-01-workflow-methodology-artifact-v0.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave38/wave38-01-workflow-methodology-artifact-v0.lisp \
  git commit -m "feat(workflow): project methodology compiles as v3 artifacts"
node scripts/verify-task-contract.mjs .missiond/tasks/wave38/wave38-01-workflow-methodology-artifact-v0.lisp
```

## Report

- `Commit hash.`
- `Exact V3 workflow artifact shape for compile_methodology write_file.`
- `What stayed DB-free and backward-compatible.`
- `Rust tests / checker fixtures added.`
- `Acceptance command results.`

