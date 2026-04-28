# Wave30 Codex Action Plan

Source: `.missiond/research/result.md`

## Codex Readout

GPT Pro's proposal is directionally correct: stop treating worker-written reports and directly edited ledgers as final truth. Wave30 should move toward an orchestrator-owned lifecycle where workers produce code plus draft evidence, while the orchestrator finalizes reports, records parent patches, writes lifecycle events, and gates completion.

The most important architectural invariant is:

```text
No task is complete unless its finalized report, final commit, parent patches, and verification receipts all agree.
```

This directly fixes the Wave29-03 drift case without giving up Wave28/Wave29 throughput gains.

## What To Adopt

### 1. Final Report Projection

Adopt the distinction between:

- worker draft report: what the worker believes happened
- finalized report: orchestrator-owned truth after worker commit, parent hotfixes, verification receipts, and batch verification

For backward compatibility, Wave30 can allow existing report paths, but the finalizer should stamp:

```lisp
:report_state finalized
:finalized_by orchestrator
:agent_commit_hash "<worker commit>"
:final_commit_hash "<latest accepted commit>"
:commit_hash "<same as final_commit_hash>"
:verified_commit_hash "<commit with valid receipts>"
:parent_patches [...]
```

### 2. Parent Hotfix Protocol

Parent hotfix is a first-class lifecycle transition, not an informal commit.

Minimum CLI surface:

```bash
node scripts/task-runner-parent-hotfix.mjs \
  --wave wave30 \
  --task wave30-03 \
  --base <previous-final-or-agent-commit> \
  --commit <parent-hotfix-commit> \
  --reason "..." \
  --finalize \
  --json
```

The command should:

- validate commit ancestry and touched paths
- record the parent patch
- invalidate or carry forward receipts conservatively
- invoke final report projection
- leave worker commits unamended

### 3. Atomic Event Log

Direct worker edits to `shared-memory.lisp` and `session-trace.lisp` should be phased out.

Adopt canonical event files:

```text
.missiond/tasks/wave30/events/index.lisp
.missiond/tasks/wave30/events/000001.event.lisp
.missiond/tasks/wave30/events/000002.event.lisp
```

Shared memory and session trace become projections. This eliminates the read-max-seq / edit-final-paren race.

### 4. Hard Dependencies vs Soft Context

Do not let context references block dispatch.

Manifest v2 should distinguish:

- `:depends_on` hard dependency
- `:context_refs` non-blocking references
- `:artifact_requires` action-specific blockers
- `:write_scope` lease constraints

Ready-queue should use only hard dependencies plus required artifacts and write leases.

### 5. Staged Source Hygiene

Add a staged guard for raw NUL bytes and accidental peer staging. This is small but high-value because raw NUL has appeared twice.

Minimum CLI:

```bash
node scripts/check-staged-source-hygiene.mjs --task <task.lisp> --manifest <manifest.lisp>
```

It should include:

- current staged scope guard behavior
- `git diff --cached --check`
- raw NUL detection on source/text files
- binary allowlist
- active lease ownership check where available

## What To Adjust

### Event Log Timing

GPT Pro puts event log as Wave30-02. That is reasonable, but it is a bigger migration than the hotfix finalizer or NUL guard. Codex recommendation:

1. implement report finalizer and parent-hotfix protocol first
2. implement staged source hygiene next if small enough
3. then build atomic event log

Rationale: Wave29-03 drift and raw NUL are already active correctness risks. Ledger contention is painful but did not corrupt final outputs yet.

### Report v2 Scope

Do not immediately force every historical report to v2. Keep v1 accepted for Wave29 and earlier. Require v2 only for Wave30+ finalized reports.

### Receipt Carry-Forward

Start conservative:

- wrong commit: invalid
- changed command: invalid
- non-zero exit: invalid
- parent patch touches receipt footprint: invalid
- unknown footprint: invalid

Carry-forward receipts can be added after a basic invalidation engine is tested.

## Proposed Wave30 Work Breakdown

### Wave30-01 Parent Hotfix Finalizer

Goal: close the Wave29-03 drift path.

Files likely owned:

- `.missiond/tasks/schema/report-contract-v2.lisp`
- `scripts/task-runner-finalize-report.mjs`
- `scripts/task-runner-parent-hotfix.mjs`
- `scripts/check-task-report.mjs`
- `scripts/verify-task-runner-batch.mjs`

Acceptance:

```bash
node scripts/check-task-report.mjs --dry-fixture
node scripts/task-runner-finalize-report.mjs --dry-fixture
node scripts/task-runner-parent-hotfix.mjs --dry-fixture
node scripts/verify-task-runner-batch.mjs --dry-fixture
node scripts/check-task-contract.mjs --all
```

Must prove:

- worker commit only finalizes cleanly
- worker commit + parent hotfix finalizes to parent commit
- multiple parent hotfixes are ordered
- finalized report before later parent patch is stale
- unrecorded post-worker commit touching task paths fails batch verification

### Wave30-02 Staged Source Hygiene

Goal: make NUL/source/index hazards automatic.

Files likely owned:

- `scripts/check-staged-source-hygiene.mjs`
- `scripts/task-scope-guard.mjs`
- `.githooks/pre-commit`
- `scripts/check-missiond-hooks.mjs`
- possibly `.missiond/tasks/schema/task-contract-v1.lisp` for `:binary-write-scope`

Acceptance:

```bash
node scripts/check-staged-source-hygiene.mjs --dry-fixture
node scripts/task-scope-guard.mjs --dry-fixture
node scripts/check-missiond-hooks.mjs --json
node scripts/check-task-contract.mjs --all
```

Must prove:

- raw NUL in `.mjs`, `.lisp`, `.rs`, `.md` staged files rejects
- declared binary fixture can be allowed
- undeclared binary file rejects
- peer task staged file rejects when lease info is available
- `git diff --cached --check` is integrated

### Wave30-03 Atomic Lifecycle Event Log

Goal: replace direct shared-memory/session-trace editing with append API + projections.

Files likely owned:

- `.missiond/tasks/schema/task-ledger-event-v1.lisp`
- `scripts/task-runner-append-event.mjs`
- `scripts/check-ledger-events.mjs`
- `scripts/project-task-ledger.mjs`
- `scripts/prepare-task-runner-wave.mjs`

Acceptance:

```bash
node scripts/task-runner-append-event.mjs --dry-fixture
node scripts/check-ledger-events.mjs --dry-fixture
node scripts/project-task-ledger.mjs --dry-fixture
node scripts/prepare-task-runner-wave.mjs --dry-fixture
```

Must prove:

- concurrent append gets gapless seq
- repeated idempotency key returns existing event
- stale lock recovery is explicit and recorded
- projections are reproducible
- manual projection drift is detected

### Wave30-04 Manifest v2 Scheduler Semantics

Goal: separate hard deps from soft context and artifact blockers.

Files likely owned:

- `.missiond/tasks/schema/task-runner-manifest-v2.lisp`
- `scripts/check-task-runner-manifest.mjs`
- `scripts/plan-task-runner.mjs`
- `scripts/render-wave-briefs.mjs`

Acceptance:

```bash
node scripts/check-task-runner-manifest.mjs --dry-fixture
node scripts/plan-task-runner.mjs --dry-fixture
node scripts/render-wave-briefs.mjs --dry-fixture
```

Must prove:

- hard dep blocks
- soft context ref does not block
- artifact requirement blocks only named phase
- write-scope lease prevents unsafe parallelism
- critical-path priority is deterministic
- default v1 group-barrier behavior remains backward compatible

### Wave30-05 Receipt v2 + Lifecycle Smoke

Goal: end-to-end proof across finalizer, event log, scheduler, source hygiene, receipts, and archive gate.

Files likely owned:

- `.missiond/tasks/schema/verification-receipt-v2.lisp`
- `scripts/check-verification-receipt.mjs`
- `scripts/verify-task-runner-batch.mjs`
- `scripts/task-runner-finalize-report.mjs`
- `scripts/task-runner-parent-hotfix.mjs`
- `scripts/task-runner-append-event.mjs`
- `scripts/plan-task-runner.mjs`

Acceptance:

```bash
node scripts/check-verification-receipt.mjs --dry-fixture
node scripts/verify-task-runner-batch.mjs --dry-fixture
node scripts/task-runner-finalize-report.mjs --dry-fixture
node scripts/task-runner-parent-hotfix.mjs --dry-fixture
node scripts/task-runner-append-event.mjs --dry-fixture
node scripts/plan-task-runner.mjs --dry-fixture
```

Must prove:

- complete task requires final-commit receipt
- local receipt cannot satisfy smoke/full
- parent patch touching footprint invalidates receipt
- carried-forward receipt explicitly cites prior evidence
- finalized report, event log, parent patches, receipts, and git commit agree

## Suggested Immediate Next Step

Before dispatching Wave30, decide whether Wave30-02 source hygiene should run before or after event log.

Codex recommendation:

```text
Wave30-01 parent-hotfix finalizer
Wave30-02 staged source hygiene
Wave30-03 atomic lifecycle event log
Wave30-04 manifest v2 hard/soft deps
Wave30-05 receipt v2 + lifecycle smoke
```

This order fixes the two active correctness bugs first, then tackles the bigger lifecycle migration.
