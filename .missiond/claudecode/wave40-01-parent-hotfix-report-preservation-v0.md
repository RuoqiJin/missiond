# wave40-01-parent-hotfix-report-preservation-v0 — parent hotfix report preservation v0

> Thin brief rendered from MissionD task-contract v1. Task Lisp remains the SSOT.
> Source: `.missiond/tasks/wave40/wave40-01-parent-hotfix-report-preservation-v0.lisp`
> Shared preamble: `.missiond/claudecode/wave40-shared-preamble.md`

## Task Contract

- kind: `code-alignment`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- verification_tier: `local`
- dispatch_group: `A`
- estimated_minutes: `45`
- heartbeat_minutes: `10`
- shared_memory: `.missiond/tasks/wave40/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave40/reports/wave40-01-parent-hotfix-report-preservation-v0.report.lisp`
- session_trace: `.missiond/tasks/wave40/session-trace.lisp` (writable)
- router_policy: `.missiond/router/router-policy-v1.lisp` (advisory / dry-run only)
- router_backend_registry: `.missiond/router/router-backend-registry-v1.lisp` (MUST NOT switch backend)
- context_atlas: `.missiond/tasks/wave40/context-atlas.lisp`
- pattern_card: `.missiond/tasks/wave40/pattern-cards.lisp`

## Context Navigation

- Read context atlas first: `.missiond/tasks/wave40/context-atlas.lisp`.
- Follow implementation pattern card: `.missiond/tasks/wave40/pattern-cards.lisp`.
- Use atlas grep anchors and pattern-card conventions before falling back to broad scans.

## Goal

Close the report-preservation gap exposed by wave39 parent hotfix finalization. The Lisp architecture says a final report is the worker report plus finalized lineage, but task-runner-finalize-report currently reconstructs a minimal report and can drop rich worker fields such as :acceptance_results, :notes, trace refs, timing notes, and optional report-contract extensions. Make parent-hotfix finalization a sparse Lisp projection that preserves existing worker report detail while adding/updating lineage fields.

## Ownership

- `.missiond/v3/missiond-blueprint.lisp`
- `.missiond/tasks/schema/report-contract-v1.lisp`
- `scripts/task-runner-finalize-report.mjs`
- `scripts/task-runner-parent-hotfix.mjs`
- `scripts/check-task-report.mjs`
- `scripts/check-v3-task-lifecycle-isomorphism.mjs`
- `scripts/verify-task-runner-batch.mjs`

## Must Not Touch

- `crates/**`
- `packages/**`
- `.missiond/v1/**`
- `.missiond/v2/**`
- `.missiond/research/**`
- `.missiond/router/**`
- `.missiond/tasks/wave28/**`
- `.missiond/tasks/wave29/**`
- `.missiond/tasks/wave30/**`
- `.missiond/tasks/wave31/**`
- `.missiond/tasks/wave32/**`
- `.missiond/tasks/wave33/**`
- `.missiond/tasks/wave34/**`
- `.missiond/tasks/wave35/**`
- `.missiond/tasks/wave36/**`
- `.missiond/tasks/wave37/**`
- `.missiond/tasks/wave38/**`
- `.missiond/tasks/wave39/**`
- `.missiond/tasks/wave40/manifest.lisp`
- `.missiond/tasks/wave40/context-atlas.lisp`
- `.missiond/tasks/wave40/pattern-cards.lisp`
- `.missiond/tasks/wave40/wave40-*.lisp`
- `.missiond/claudecode/**`

## Requirements

1. Update Lisp first. In .missiond/v3/missiond-blueprint.lisp and .missiond/tasks/schema/report-contract-v1.lisp, state that parent-hotfix finalization is a sparse report projection: it preserves the worker report's existing non-lineage fields and only patches final lineage fields unless an explicit replacement option is supplied.
2. Fix scripts/task-runner-finalize-report.mjs so finalizeReportSource/finalizeReportFile keep worker report detail. Existing :acceptance_results must be preserved by default. If --acceptance-command is supplied, append that verification result rather than replacing worker acceptance unless an explicit replacement mode already exists and is documented.
3. Preserve optional report-contract fields that are already present in the worker report, at least :notes, :verification_tier, :time_sinks, :major_decisions, :unexpected_work, :blockers, :trace_refs, router recommendation/readiness/dispatch fields, and verification receipt fields. Prefer a generic Lisp property preservation path over hand-copying only this list.
4. Make task-runner-parent-hotfix.mjs use the preservation path. The helper should still be read-only by default and should only mutate report bytes when --write-report is supplied.
5. Add a dry fixture reproducing the wave39 class: a worker report with multiple acceptance results and notes is finalized with a parent patch; the final report must keep those acceptance entries and notes while adding :agent_commit_hash, :final_commit_hash, :verified_commit_hash, and :parent_patches.
6. Add or update checker/smoke coverage so verify-task-runner-batch and check-v3-task-lifecycle-isomorphism pin the preservation contract. Backward compatibility for minimal old reports must remain green.
7. Do not rewrite historical reports or wave39 artifacts in this task. The wave39 report is only evidence for a fixture; code and schema should be fixed forward.

## Acceptance Commands

```bash
node scripts/task-runner-finalize-report.mjs --dry-fixture
node scripts/task-runner-parent-hotfix.mjs --dry-fixture
node scripts/check-task-report.mjs --dry-fixture
node scripts/verify-task-runner-batch.mjs --dry-fixture
node scripts/check-v3-task-lifecycle-isomorphism.mjs --dry-fixture
node scripts/check-v3-task-lifecycle-isomorphism.mjs
node scripts/check-lisp-blueprint-compression.mjs
node scripts/check-architecture-lisp.mjs --no-structure .missiond/v3/missiond-blueprint.lisp
perl -ne 'exit 1 if /\x00/' .missiond/v3/missiond-blueprint.lisp .missiond/tasks/schema/report-contract-v1.lisp scripts/task-runner-finalize-report.mjs scripts/task-runner-parent-hotfix.mjs scripts/check-task-report.mjs scripts/check-v3-task-lifecycle-isomorphism.mjs scripts/verify-task-runner-batch.mjs
git diff --check -- .missiond/v3/missiond-blueprint.lisp .missiond/tasks/schema/report-contract-v1.lisp scripts/task-runner-finalize-report.mjs scripts/task-runner-parent-hotfix.mjs scripts/check-task-report.mjs scripts/check-v3-task-lifecycle-isomorphism.mjs scripts/verify-task-runner-batch.mjs
```

## Shared Protocol

Read `.missiond/claudecode/wave40-shared-preamble.md` once for shared-memory, report, session-trace, router, hook, commit, and verifier protocol. Do not paste or duplicate that boilerplate into this task.
- Task-specific scope and acceptance above override generic guidance.
- Load the context atlas / pattern card before broad repository search; use their anchors to reduce navigation misses.
- Append coordination facts to shared memory when present; write the report contract when the task completes.
- If work is still active after 10 minutes without a completion, append a heartbeat/observation entry or report a blocker.

## Commit

Commit only files inside the declared write scope after acceptance:

```bash
git add ".missiond/v3/missiond-blueprint.lisp" \
        ".missiond/tasks/schema/report-contract-v1.lisp" \
        "scripts/task-runner-finalize-report.mjs" \
        "scripts/task-runner-parent-hotfix.mjs" \
        "scripts/check-task-report.mjs" \
        "scripts/check-v3-task-lifecycle-isomorphism.mjs" \
        "scripts/verify-task-runner-batch.mjs"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave40/wave40-01-parent-hotfix-report-preservation-v0.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave40/wave40-01-parent-hotfix-report-preservation-v0.lisp \
  git commit -m "feat(tasks): preserve hotfix report detail"
node scripts/verify-task-contract.mjs .missiond/tasks/wave40/wave40-01-parent-hotfix-report-preservation-v0.lisp
```

## Report

- `Commit hash.`
- `Lisp/report-contract wording added before code changes.`
- `Preservation implementation shape: sparse AST/property patch vs minimal reconstruction.`
- `What fields are preserved and what lineage fields are patched.`
- `Wave39-style preservation fixture result.`
- `Acceptance command results.`

