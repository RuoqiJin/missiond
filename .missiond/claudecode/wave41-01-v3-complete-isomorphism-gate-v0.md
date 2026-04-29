# wave41-01-v3-complete-isomorphism-gate-v0 — v3 complete isomorphism gate v0

> Thin brief rendered from MissionD task-contract v1. Task Lisp remains the SSOT.
> Source: `.missiond/tasks/wave41/wave41-01-v3-complete-isomorphism-gate-v0.lisp`
> Shared preamble: `.missiond/claudecode/wave41-shared-preamble.md`

## Task Contract

- kind: `code-alignment`
- owner: `claudecode`
- dispatch_strategy: `fresh-code-alignment`
- verification_tier: `local`
- dispatch_group: `A`
- estimated_minutes: `45`
- heartbeat_minutes: `10`
- shared_memory: `.missiond/tasks/wave41/shared-memory.lisp`
- report_contract: `.missiond/tasks/wave41/reports/wave41-01-v3-complete-isomorphism-gate-v0.report.lisp`
- session_trace: `.missiond/tasks/wave41/session-trace.lisp` (writable)
- router_policy: `.missiond/router/router-policy-v1.lisp` (advisory / dry-run only)
- router_backend_registry: `.missiond/router/router-backend-registry-v1.lisp` (MUST NOT switch backend)
- context_atlas: `.missiond/tasks/wave41/context-atlas.lisp`
- pattern_card: `.missiond/tasks/wave41/pattern-cards.lisp`

## Context Navigation

- Read context atlas first: `.missiond/tasks/wave41/context-atlas.lisp`.
- Follow implementation pattern card: `.missiond/tasks/wave41/pattern-cards.lisp`.
- Use atlas grep anchors and pattern-card conventions before falling back to broad scans.

## Goal

Turn the current collection of V3 Lisp/code isomorphism checks into an explicit completion gate. Right now every per-surface V3 checker passes, but the blueprint implementation-map still labels six surfaces as code-aligned-partial and some checkers still require that partial status string. Graduate the implementation-map to code-aligned where the live checkers prove the Lisp contract, and add a single aggregate checker that fails if any implementation-map surface regresses to partial or if any per-surface checker fails.

## Ownership

- `.missiond/v3/missiond-blueprint.lisp`
- `scripts/check-v3-code-isomorphism-complete.mjs`
- `scripts/check-v3-request-lisp-isomorphism.mjs`
- `scripts/check-v3-intent-alignment-isomorphism.mjs`
- `scripts/check-v3-plan-execution-isomorphism.mjs`
- `scripts/check-v3-workflow-isomorphism.mjs`
- `scripts/check-v3-task-lifecycle-isomorphism.mjs`
- `scripts/check-v3-workstation-config-isomorphism.mjs`
- `scripts/check-lisp-blueprint-compression.mjs`

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
- `.missiond/tasks/wave40/**`
- `.missiond/tasks/wave41/manifest.lisp`
- `.missiond/tasks/wave41/context-atlas.lisp`
- `.missiond/tasks/wave41/pattern-cards.lisp`
- `.missiond/tasks/wave41/wave41-*.lisp`
- `.missiond/claudecode/**`

## Requirements

1. Update .missiond/v3/missiond-blueprint.lisp first. In the implementation-map, graduate the six V3 surfaces that are currently code-aligned-partial to code-aligned only if the existing live checker for that surface proves the current note/code contract. Do not weaken the notes.
2. Add scripts/check-v3-code-isomorphism-complete.mjs as an aggregate completion gate. It should be read-only, deterministic, support --json and --dry-fixture, and fail when any expected implementation-map surface is missing, any implementation-map surface still has :status "code-aligned-partial", any expected surface lacks :status "code-aligned", :code, or :note, the compression-contract omits this aggregate command, or any per-surface V3 checker fails.
3. The aggregate checker should cover exactly these implementation surfaces unless the blueprint explicitly changes the V3 surface set: mission_request, mission_directive, mission_plan, mission_workflow, task-runner-cli, workstation-config.
4. Update all per-surface checkers that currently require code-aligned-partial so they now pin code-aligned and adjust their dry fixtures accordingly. Checkers that did not pin the status should stay compatible but may add a code-aligned needle if that improves the gate.
5. Add the aggregate command to the V3 compression-contract :checks list. If check-lisp-blueprint-compression needs to pin that command, update it narrowly.
6. Do not edit Rust or frontend code in this task. This is a Lisp/checker graduation task after the implementation work from waves 31-40.

## Acceptance Commands

```bash
node scripts/check-v3-code-isomorphism-complete.mjs --dry-fixture
node scripts/check-v3-code-isomorphism-complete.mjs
node scripts/check-v3-request-lisp-isomorphism.mjs --dry-fixture
node scripts/check-v3-request-lisp-isomorphism.mjs
node scripts/check-v3-intent-alignment-isomorphism.mjs --dry-fixture
node scripts/check-v3-intent-alignment-isomorphism.mjs
node scripts/check-v3-plan-execution-isomorphism.mjs --dry-fixture
node scripts/check-v3-plan-execution-isomorphism.mjs
node scripts/check-v3-workflow-isomorphism.mjs --dry-fixture
node scripts/check-v3-workflow-isomorphism.mjs
node scripts/check-v3-task-lifecycle-isomorphism.mjs --dry-fixture
node scripts/check-v3-task-lifecycle-isomorphism.mjs
node scripts/check-v3-workstation-config-isomorphism.mjs --dry-fixture
node scripts/check-v3-workstation-config-isomorphism.mjs
node scripts/check-lisp-blueprint-compression.mjs
node scripts/check-architecture-lisp.mjs --no-structure .missiond/v3/missiond-blueprint.lisp
perl -ne 'exit 1 if /\x00/' .missiond/v3/missiond-blueprint.lisp scripts/check-v3-code-isomorphism-complete.mjs scripts/check-v3-request-lisp-isomorphism.mjs scripts/check-v3-intent-alignment-isomorphism.mjs scripts/check-v3-plan-execution-isomorphism.mjs scripts/check-v3-workflow-isomorphism.mjs scripts/check-v3-task-lifecycle-isomorphism.mjs scripts/check-v3-workstation-config-isomorphism.mjs scripts/check-lisp-blueprint-compression.mjs
git diff --check -- .missiond/v3/missiond-blueprint.lisp scripts/check-v3-code-isomorphism-complete.mjs scripts/check-v3-request-lisp-isomorphism.mjs scripts/check-v3-intent-alignment-isomorphism.mjs scripts/check-v3-plan-execution-isomorphism.mjs scripts/check-v3-workflow-isomorphism.mjs scripts/check-v3-task-lifecycle-isomorphism.mjs scripts/check-v3-workstation-config-isomorphism.mjs scripts/check-lisp-blueprint-compression.mjs
```

## Shared Protocol

Read `.missiond/claudecode/wave41-shared-preamble.md` once for shared-memory, report, session-trace, router, hook, commit, and verifier protocol. Do not paste or duplicate that boilerplate into this task.
- Task-specific scope and acceptance above override generic guidance.
- Load the context atlas / pattern card before broad repository search; use their anchors to reduce navigation misses.
- Append coordination facts to shared memory when present; write the report contract when the task completes.
- If work is still active after 10 minutes without a completion, append a heartbeat/observation entry or report a blocker.

## Commit

Commit only files inside the declared write scope after acceptance:

```bash
git add ".missiond/v3/missiond-blueprint.lisp" \
        "scripts/check-v3-code-isomorphism-complete.mjs" \
        "scripts/check-v3-request-lisp-isomorphism.mjs" \
        "scripts/check-v3-intent-alignment-isomorphism.mjs" \
        "scripts/check-v3-plan-execution-isomorphism.mjs" \
        "scripts/check-v3-workflow-isomorphism.mjs" \
        "scripts/check-v3-task-lifecycle-isomorphism.mjs" \
        "scripts/check-v3-workstation-config-isomorphism.mjs" \
        "scripts/check-lisp-blueprint-compression.mjs"
node scripts/task-scope-guard.mjs --task .missiond/tasks/wave41/wave41-01-v3-complete-isomorphism-gate-v0.lisp --mode staged
MISSIOND_TASK_CONTRACT=.missiond/tasks/wave41/wave41-01-v3-complete-isomorphism-gate-v0.lisp \
  git commit -m "feat(v3): add complete code-isomorphism gate"
node scripts/verify-task-contract.mjs .missiond/tasks/wave41/wave41-01-v3-complete-isomorphism-gate-v0.lisp
```

## Report

- `Commit hash.`
- `Which implementation-map surfaces were graduated and why.`
- `Aggregate checker contract and dry-fixture cases.`
- `Per-surface checker updates.`
- `Acceptance command results.`

