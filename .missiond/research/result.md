# Wave30 Orchestrator Architecture Proposal

The research pack makes the core diagnosis clear: Wave28 and Wave29 solved much of the prompt-size and worker-efficiency problem, but exposed a deeper lifecycle problem: reports, ledgers, parent patches, receipts, and scheduling state are still split across actors without a single orchestrator-owned state transition model. The concrete failures were parent-hotfix report drift, shared-ledger sequence races, conflation of hard dependencies with soft context references, repeated raw NUL-byte source corruption, and ambiguity over who owns final truth after a worker exits. 

## 1. Concise diagnosis

MissionD’s current system is **efficient but under-specified at the lifecycle boundary**.

Wave28 and Wave29 proved that thin briefs, shared preambles, verification tiers, productive-only dispatch, context atlases, pattern cards, ready queues, and verification receipts materially improve throughput. The system is no longer primarily bottlenecked by prompt bulk or individual worker speed. It is bottlenecked by **coordination semantics**.

The key architectural problem is that several files currently behave as if they are authoritative at the same time:

* the manifest says what should happen;
* the worker report says what the worker thinks happened;
* shared-memory and session-trace say what was observed;
* git history says what actually changed;
* verification receipts say what evidence exists;
* parent hotfixes may alter final reality after the worker is gone.

That is why Wave29-03 drifted: the worker report was written before the parent hotfix existed, and no post-worker actor was required to reconcile the report with the final commit lineage. The existing lineage schema works when one actor makes all commits, but fails when the parent orchestrator patches after worker exit. 

The Wave30 fix should not be another patch. It should make task execution an orchestrator-owned lifecycle:

```text
manifest
  -> dispatch
  -> claim
  -> worker implementation
  -> worker commit
  -> optional parent patch(es)
  -> verification
  -> report finalization
  -> completion
  -> archive
```

Workers still implement code and write draft reports, but the orchestrator owns the final state.

---

# 2. Target architecture

## Name: Event-Sourced Task Runner with Finalized Report Projection

The target architecture should have three authoritative layers:

```text
Plan truth:
  contracts + manifest

Lifecycle truth:
  append-only task event log

Outcome truth:
  finalized reports + verification receipts
```

Markdown briefs, shared-memory views, session traces, archive indexes, and backfill outputs are generated projections, not authoritative state.

## Recommended implementation boundary

Keep Wave30 as **Node CLIs over Lisp files**, not a daemon rewrite.

The daemon may expose dry-run/status surfaces later, but the canonical Wave30 implementation should remain deterministic local tooling:

* no network;
* no LLM calls;
* no hidden git mutation;
* deterministic checkers;
* additive migration from existing Wave28/Wave29 behavior;
* Lisp contracts, manifests, reports, receipts, and event records remain machine-readable SSOT artifacts.

A future daemon can wrap the same commands, but the CLI/event schema should be the stable interface.

---

# 3. Named components and responsibilities

| Component                         | Owner                                   | Responsibility                                                                                                   |
| --------------------------------- | --------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `task-runner prepare`             | Orchestrator                            | Creates contracts, manifest, artifact directories, initial lifecycle event log.                                  |
| `task-runner render-briefs`       | Orchestrator                            | Generates Markdown briefs from contracts, manifest, context atlas, pattern cards. Briefs are non-authoritative.  |
| `task-runner dispatch`            | Orchestrator                            | Computes ready queue, grants leases, enforces hard deps, concurrency, write-scope overlap, and backpressure.     |
| `task-runner append-event`        | Orchestrator API used by workers/parent | Atomically appends lifecycle events. No direct worker edits to shared-memory/session-trace.                      |
| `task-runner claim` / `heartbeat` | Worker through append API               | Records claim, lease, progress, and liveness events.                                                             |
| `task-runner staged-guard`        | Worker and parent                       | Enforces write scope, must-not-touch, staged path ownership, `git diff --check`, and NUL-byte hygiene.           |
| `task-runner worker-handoff`      | Worker                                  | Validates worker commit, draft report, local receipts, and handoff completeness.                                 |
| `task-runner parent-hotfix`       | Parent orchestrator                     | Records a post-worker parent patch, invalidates or carries forward receipts, triggers report finalization.       |
| `task-runner verify`              | Verifier/orchestrator                   | Runs deterministic verification commands and emits verification receipts.                                        |
| `task-runner finalize-report`     | Orchestrator                            | Produces the authoritative final report from draft report + event log + git lineage + receipts.                  |
| `task-runner batch-verify`        | Verifier/orchestrator                   | Checks lifecycle invariants across manifest, reports, receipts, event log, shared projections, and git.          |
| `task-runner archive`             | Orchestrator                            | Archives finalized, verified task outputs. Archive/backfill/index remain orchestrator-owned, never worker tasks. |

---

# 4. Core artifact lifecycle

## Artifact ownership

| Artifact               |               Authoritative? | Writer                                                 |
| ---------------------- | ---------------------------: | ------------------------------------------------------ |
| Task contract          |                          Yes | Orchestrator                                           |
| Manifest               |                          Yes | Orchestrator                                           |
| Markdown brief         |                           No | Renderer                                               |
| Context atlas          | Yes, for navigation metadata | Orchestrator or assigned productive task, then checked |
| Pattern card           |    Yes, for routing metadata | Orchestrator or assigned productive task, then checked |
| Event log              |   Yes, for lifecycle history | Append API only                                        |
| Shared memory          |                   Projection | Append API / projector                                 |
| Session trace          |                   Projection | Append API / projector                                 |
| Worker draft report    |               No, draft only | Worker                                                 |
| Final report           |                          Yes | Orchestrator finalizer                                 |
| Verification receipt   |            Yes, for evidence | Verifier through receipt writer                        |
| Archive/backfill/index |            Yes after archive | Orchestrator only                                      |

## Draft report versus final report

Wave30 should explicitly split worker output from final truth.

Workers may write:

```text
.missiond/tasks/wave30/reports/drafts/wave30-03.report.lisp
```

The finalizer writes:

```text
.missiond/tasks/wave30/reports/wave30-03.report.lisp
```

For backward compatibility, Wave30 can initially allow workers to write the existing report path, but `finalize-report` must normalize it into a finalized report with:

```lisp
:report_state :finalized
:finalized_by :orchestrator
```

Only finalized reports count for task completion, batch verification, archive, or successor dependency satisfaction.

---

# 5. Formal state and event model

## Task states

Task state is derived from ordered lifecycle events, not manually edited state.

```text
PLANNED
  -> BRIEF_RENDERED
  -> DISPATCHABLE
  -> DISPATCHED
  -> CLAIMED
  -> RUNNING
  -> WORKER_COMMITTED
  -> DRAFT_REPORTED
  -> PATCH_PENDING? / PARENT_PATCHED*
  -> VERIFICATION_PENDING
  -> VERIFIED
  -> REPORT_FINALIZED
  -> COMPLETE
  -> ARCHIVED
```

Exceptional states:

```text
BLOCKED
STALE
FAILED
ABANDONED
SUPERSEDED
```

A task is not complete merely because a worker committed code. It is complete only when the final report is aligned with the final commit and valid verification receipts exist.

## Minimal event schema

Use standalone Lisp event files, one event per file, rather than editing a single list with a trailing paren.

Example:

```lisp
(:task_event
  :schema "task-ledger-event-v1"
  :seq 42
  :event_id "evt-wave30-000042-7f3c2a"
  :wave_id "wave30"
  :task_id "wave30-03"
  :kind :worker_commit
  :actor (:role :worker :id "worker-a")
  :attempt_id "attempt-wave30-03-01"
  :time "2026-04-28T15:32:10-07:00"
  :payload
    (:commit_hash "d36de8040bf0"
     :base_commit_hash "abc1234"
     :touched_paths
       ("scripts/prepare-task-runner-wave.mjs"
        ".missiond/tasks/wave30/reports/drafts/wave30-03.report.lisp")
     :verification_tier :local)
  :idempotency_key "wave30-03/worker_commit/d36de8040bf0")
```

Recommended path:

```text
.missiond/tasks/wave30/events/000042.event.lisp
```

Maintain a small index:

```text
.missiond/tasks/wave30/events/index.lisp
```

Example:

```lisp
(:event_index
  :schema "task-ledger-index-v1"
  :wave_id "wave30"
  :next_seq 43
  :last_event_id "evt-wave30-000042-7f3c2a"
  :idempotency_keys
    (("wave30-03/worker_commit/d36de8040bf0" . "evt-wave30-000042-7f3c2a")))
```

## Event kinds

Core event kinds:

```lisp
:planned
:brief_rendered
:dispatchable
:dispatched
:claimed
:preamble_read
:heartbeat
:blocked
:unblocked
:worker_commit
:draft_report_written
:parent_hotfix
:verification_receipt
:report_finalized
:complete
:archived
:lease_expired
:abandoned
```

## Commit lineage model

The current lineage fields should be given strict meanings.

| Field                   | Meaning                                                                                                                                  |
| ----------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| `:agent_commit_hash`    | The first accepted commit produced by the assigned worker for the task. Immutable once recorded.                                         |
| `:final_commit_hash`    | The latest accepted task commit after worker self-fixes and parent hotfixes. This is the commit whose tree represents final task output. |
| `:commit_hash`          | Backward-compatible alias for `:final_commit_hash` in finalized reports. In finalized reports, these must be equal.                      |
| `:verified_commit_hash` | The commit for which required verification is valid. For complete tasks, this must equal `:final_commit_hash`.                           |
| `:parent_patches`       | Ordered list of parent-orchestrator patch commits after worker handoff.                                                                  |

Final report example:

```lisp
(:task_report
  :schema "report-contract-v2"
  :task_id "wave30-03"
  :report_state :finalized

  :agent_commit_hash "d36de8040bf0"
  :commit_hash "d842b1d9f4aa"
  :final_commit_hash "d842b1d9f4aa"
  :verified_commit_hash "d842b1d9f4aa"

  :parent_patches
    ((:patch_id "patch-wave30-03-001"
      :base_commit_hash "d36de8040bf0"
      :commit_hash "d842b1d9f4aa"
      :actor :parent_orchestrator
      :reason "Fix report lineage and staged hygiene issue after worker exit."
      :touched_paths
        (".missiond/tasks/wave30/reports/wave30-03.report.lisp"
         "scripts/prepare-task-runner-wave.mjs")
      :event_id "evt-wave30-000057-a91bce"))

  :receipt_ids
    ("vr-wave30-03-smoke-001")

  :finalized_by :orchestrator
  :finalized_event_id "evt-wave30-000061-41ac9e")
```

## Report states

```text
NONE
DRAFT
STALE
FINALIZED
ARCHIVED
```

A report becomes `STALE` whenever a later `:parent_hotfix` event exists for the same task and the report’s `:finalized_event_id` precedes it.

## Receipt states

```text
FRESH
REUSED_SAME_COMMIT
CARRIED_FORWARD
INVALIDATED
ESCALATED
```

A carried-forward receipt is not the same as pretending the old receipt verified the new commit. It is a new receipt at the final commit that cites older evidence and records why reuse is safe.

---

# 6. Scheduler semantics

## Dependency taxonomy

Wave30 should separate four concepts that are currently too easy to conflate.

### 1. Hard dependencies

A hard dependency blocks dispatch.

```lisp
:depends_on ("wave30-01")
```

A hard dependency is satisfied only when the predecessor reaches:

```text
REPORT_FINALIZED or COMPLETE
```

Usually `REPORT_FINALIZED` is enough for successors; `ARCHIVED` is not required.

### 2. Soft context references

A soft reference affects brief content and priority, but never blocks dispatch.

```lisp
:context_refs
  ((:task_id "wave30-02"
    :reason "Useful pattern-card examples."
    :required false))
```

If the referenced task is complete, its final report and artifacts are included in the brief. If not, the brief says the context is pending.

### 3. Artifact requirements

An artifact requirement blocks only the named action, not necessarily the whole task.

```lisp
:artifact_requires
  ((:artifact :context_atlas
    :producer "wave30-01"
    :blocks (:brief_render :implementation))

   (:artifact :verification_receipt_schema
    :producer "wave30-05"
    :blocks (:verification)))
```

This allows a task to start design or scaffolding while deferring a later action that truly needs the artifact.

### 4. Write-scope constraints

Write-scope overlap affects parallel eligibility, not logical dependency.

```lisp
:write_scope
  ("scripts/plan-task-runner.mjs"
   ".missiond/tasks/wave30/reports/**")

:must_not_touch
  ("src/daemon/**")
```

Two tasks may run in parallel only if their active write leases do not overlap, unless an explicit exception exists.

## Ready-queue algorithm

Use hard dependencies only for readiness.

A task is ready when:

```text
all hard deps are finalized or complete
AND required artifacts for dispatch are available
AND no must-not-touch violation exists
AND no active write-scope lease conflicts
AND concurrency tokens are available
```

Soft context references never block readiness.

## Priority function

Use deterministic critical-path priority:

```text
priority(task) =
  hard_dep_critical_path_minutes descending
  + estimated_minutes descending
  + soft_context_available_bonus
  + ready_since_seq ascending
  + lexical_task_id ascending
```

Critical path should be computed from hard dependencies only:

```text
critical_path(task) =
  estimated_minutes(task)
  + max(critical_path(successor) for hard-dep successors)
```

Soft references may add a small tie-breaker bonus but must never convert into blocking edges.

## Backpressure

Backpressure should be explicit resource accounting:

```lisp
:runner_limits
  (:max_parallel_workers 3
   :max_parallel_commits 1
   :max_parallel_full_verifications 1
   :max_parallel_report_finalizers 1)
```

Recommended tokens:

| Token              | Purpose                                                 |
| ------------------ | ------------------------------------------------------- |
| `worker`           | Limits active workers.                                  |
| `git_index`        | Serializes staging/commit windows in a shared worktree. |
| `full_verifier`    | Prevents expensive full checks from stampeding.         |
| `ledger_writer`    | Internally serialized by append API.                    |
| `report_finalizer` | Avoids conflicting final report rewrites.               |

## Lease model

Dispatch creates a lease:

```lisp
(:task_event
  :kind :dispatched
  :task_id "wave30-03"
  :payload
    (:lease_id "lease-wave30-03-001"
     :write_scope_hash "sha256:..."
     :expires_after_minutes 20
     :tokens (:worker)
     :expected_heartbeat_minutes 5))
```

If heartbeats stop:

```text
now > last_heartbeat + 2 * heartbeat_minutes
```

the task becomes `STALE`. The scheduler should not dispatch dependent tasks until the stale lease is resolved, expired, or abandoned.

---

# 7. Parent-hotfix protocol

The parent-hotfix protocol is the most important Wave30 change.

## Goal

A parent hotfix after worker exit must never leave the final report pointing at the worker commit while final bytes live at a later parent commit.

## Protocol

### Step 1: Worker hands off

Worker emits:

```lisp
:worker_commit
:draft_report_written
```

The worker draft report may reference the worker commit.

### Step 2: Parent applies hotfix

The parent makes a normal git commit. The commit is not an amend.

Then the parent records it:

```bash
node scripts/task-runner-parent-hotfix.mjs \
  --wave wave30 \
  --task wave30-03 \
  --base d36de8040bf0 \
  --commit d842b1d9f4aa \
  --reason "Fix lineage drift after worker exit." \
  --json
```

This command should not need to mutate git. It records an existing commit and updates orchestrator-owned artifacts.

### Step 3: Command validates patch

The command checks:

```text
commit is descendant of previous final commit
commit touches declared task scope, report path, or explicit parent override path
commit is not already recorded
task is not archived
draft/final report will become stale unless finalized
```

### Step 4: Append parent-hotfix event

```lisp
(:task_event
  :schema "task-ledger-event-v1"
  :seq 57
  :kind :parent_hotfix
  :task_id "wave30-03"
  :actor (:role :parent_orchestrator)
  :payload
    (:patch_id "patch-wave30-03-001"
     :base_commit_hash "d36de8040bf0"
     :commit_hash "d842b1d9f4aa"
     :reason "Fix lineage drift after worker exit."
     :touched_paths
       ("scripts/prepare-task-runner-wave.mjs"
        ".missiond/tasks/wave30/reports/wave30-03.report.lisp")))
```

### Step 5: Invalidate or carry forward receipts

All receipts for the task are re-evaluated.

A receipt is invalidated if the parent patch touches:

* any path in the receipt footprint;
* any checker/verifier source used by the receipt;
* the task contract or manifest fields relevant to that receipt;
* the final report, when the receipt command checked the report;
* any unknown or global dependency footprint.

If unaffected, the verifier may emit a carry-forward receipt at the new final commit.

### Step 6: Finalize report

`task-runner-parent-hotfix` should invoke:

```bash
node scripts/task-runner-finalize-report.mjs \
  --wave wave30 \
  --task wave30-03 \
  --json
```

The finalizer rewrites the authoritative final report so that:

```lisp
:agent_commit_hash   ; remains original worker commit
:final_commit_hash   ; becomes latest parent hotfix commit
:commit_hash         ; equals final_commit_hash
:verified_commit_hash ; equals final_commit_hash only after valid verification
:parent_patches      ; includes every parent patch in order
```

### Step 7: Batch verifier enforces alignment

`verify-task-runner-batch` must fail if:

```text
latest lineage event != report final_commit_hash
report commit_hash != report final_commit_hash
verified_commit_hash != final_commit_hash for a complete task
parent_hotfix events are missing from parent_patches
report finalized before latest parent_hotfix event
unrecorded post-worker commits touch task output paths
```

This gives two protections:

1. the parent-hotfix command updates reports immediately;
2. batch verification catches any parent patch that bypassed the protocol.

---

# 8. Atomic ledger append protocol

The current read-max-seq/edit-before-final-paren model should be retired.

## Recommended design

Use an append API with a lock and one standalone event file per event.

```bash
node scripts/task-runner-append-event.mjs \
  --wave wave30 \
  --task wave30-03 \
  --kind heartbeat \
  --actor-role worker \
  --actor-id worker-a \
  --payload @payload.lisp \
  --idempotency-key wave30-03/heartbeat/attempt-01/0003 \
  --json
```

## Files

```text
.missiond/tasks/wave30/events/
  index.lisp
  000001.event.lisp
  000002.event.lisp
  000003.event.lisp

.missiond/tasks/wave30/projections/
  shared-memory.lisp
  session-trace.lisp
```

The existing shared-memory and session-trace files can remain, but they become projections of the canonical event log.

## Append algorithm

Under a filesystem lock:

```text
1. Validate event payload against schema.
2. Validate actor may emit this event kind.
3. Read events/index.lisp.
4. Check idempotency key.
5. Allocate seq = next_seq.
6. Create temp event file.
7. fsync temp file.
8. Atomic rename to 000NNN.event.lisp.
9. Update index.lisp with next_seq + 1.
10. Regenerate or incrementally update projections.
11. Release lock.
```

Use a lock directory rather than a final-paren edit:

```text
.missiond/tasks/wave30/events/.append.lock/
```

Creating a directory is atomic on normal local filesystems. If a stale lock is detected, recovery must be explicit:

```bash
node scripts/task-runner-append-event.mjs --recover-lock --wave wave30
```

Recovery itself should append a `:lock_recovered` event after acquiring the new lock.

## Guarantees

The append API guarantees:

```text
gapless seq numbers
no duplicate idempotency keys
no manual ledger edits
no concurrent final-paren conflicts
deterministic projections
schema validation before write
actor permission checks
```

Workers should never directly edit shared-memory or session-trace again.

---

# 9. Verification receipt rules

## Required receipt fields

```lisp
(:verification_receipt
  :schema "verification-receipt-v2"
  :receipt_id "vr-wave30-03-smoke-001"
  :task_id "wave30-03"
  :tier :smoke
  :command
    (:argv ("node" "scripts/verify-task-run.mjs"
            "--task" "wave30-03"
            "--tier" "smoke")
     :cwd "."
     :env_fingerprint "sha256:...")
  :exit_code 0
  :evidence_commit_hash "d842b1d9f4aa"
  :verified_commit_hash "d842b1d9f4aa"
  :path_fingerprints
    ((:path "scripts/prepare-task-runner-wave.mjs"
      :sha256 "...")
     (:path ".missiond/tasks/wave30/reports/wave30-03.report.lisp"
      :sha256 "..."))
  :verifier_fingerprint
    (:path "scripts/verify-task-run.mjs"
     :sha256 "...")
  :mode :fresh
  :created_event_id "evt-wave30-000060-19f2ba")
```

## Reuse rules

A receipt may be reused directly only when all are true:

```text
same verified commit
same normalized command
same tier or declared tier subsumption
same relevant file fingerprints
same verifier/checker fingerprints
exit_code == 0
receipt schema is current or accepted by migration shim
```

## Tier rules

```text
local cannot satisfy smoke
local cannot satisfy full
smoke cannot satisfy full
full may satisfy smoke/local only if the receipt explicitly declares that capability
```

Better than relying on tier names alone, receipts should eventually include:

```lisp
:satisfies (:local :smoke)
```

## Parent-hotfix invalidation

A parent hotfix invalidates a receipt unless the verifier can prove the receipt footprint is unaffected.

Invalidated when the patch touches:

```text
receipt path footprint
checker/verifier source
task contract
manifest scheduling or verification fields
final report checked by the command
schema files used by the command
unknown dependency footprint
```

## Carry-forward rule

If a parent patch does not affect the receipt footprint, the verifier may emit a new receipt:

```lisp
(:verification_receipt
  :schema "verification-receipt-v2"
  :receipt_id "vr-wave30-03-smoke-002"
  :task_id "wave30-03"
  :tier :smoke
  :mode :carried_forward
  :evidence_commit_hash "d36de8040bf0"
  :verified_commit_hash "d842b1d9f4aa"
  :input_receipts ("vr-wave30-03-smoke-001")
  :carry_forward_reason
    (:changed_paths_since_evidence
      ("docs/wave30-note.md")
     :receipt_footprint_unaffected true))
```

The final report should only use receipts whose `:verified_commit_hash` equals the final commit.

## Escalation rules

Escalate verification when:

```text
receipt footprint is unknown
parent patch touches checker/verifier source
parent patch touches schema
parent patch changes scheduler semantics
parent patch changes report lineage fields
task has multiple parent patches and no fresh post-patch receipt
local receipt is being asked to satisfy smoke/full
```

---

# 10. Source hygiene rules

Wave30 should add a staged source hygiene guard, not rely on workers noticing binary behavior from `rg` or grep.

## Command

```bash
node scripts/check-staged-source-hygiene.mjs \
  --task wave30-03 \
  --manifest .missiond/tasks/wave30/manifest.lisp
```

This should run before worker commits, parent hotfix records, and batch verification.

## Rules

### 1. Staged scope guard

Reject staged paths outside:

```text
task write_scope
task report draft/final path
allowed receipt path
allowed event/projection path written by append API
```

Reject all `must_not_touch` paths.

### 2. No accidental peer staging

In a shared worktree, the guard should fail if staged files belong to another active task lease.

Workers should stage by pathspec, never `git add -A`.

### 3. NUL-byte guard

For staged text/source paths, reject any byte `0x00`.

Apply to at least:

```text
*.mjs
*.js
*.ts
*.lisp
*.md
*.json
*.yaml
*.yml
*.toml
*.rs
*.py
*.sh
*.txt
```

### 4. Binary allowlist

Binary files are allowed only when declared in the contract or manifest:

```lisp
:binary_write_scope
  ("fixtures/*.png")
```

Otherwise, staged binary-like content is rejected.

### 5. Whitespace guard

Run:

```bash
git diff --cached --check
```

as part of the same guard.

### 6. Final tree scan

Batch verification should also scan tracked source files at the final commit for raw NUL bytes. This catches cases where the hook was bypassed.

---

# 11. Invariants

## Lifecycle invariants

| Invariant                                                          | Enforced by                     |
| ------------------------------------------------------------------ | ------------------------------- |
| Every lifecycle event has a unique, gapless `:seq`.                | `check-ledger-events`           |
| Workers cannot directly edit lifecycle ledgers.                    | staged guard + batch verifier   |
| A task cannot complete before final report finalization.           | `check-task-lifecycle`          |
| A task cannot archive before complete.                             | archive command                 |
| Successors depend on finalized hard deps, not draft reports.       | scheduler + batch verifier      |
| Soft context refs never block dispatch.                            | scheduler tests                 |
| Parent patches after worker handoff must appear in report lineage. | finalizer + batch verifier      |
| Finalized report `:commit_hash` equals `:final_commit_hash`.       | `check-task-report`             |
| Complete task `:verified_commit_hash` equals `:final_commit_hash`. | `check-task-report`             |
| Receipts for complete tasks verify the final commit.               | `check-verification-receipt`    |
| Raw NUL bytes are rejected from staged source files.               | staged hygiene guard            |
| Archive/backfill/index are orchestrator-owned.                     | manifest checker + staged guard |

---

# 12. Failure-handling table

| Failure                                                 | Handling                                                                                                                 |
| ------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------ |
| Worker exits after commit but before final report       | Orchestrator finalizer creates or updates final report from draft + events; if draft is missing, task becomes `BLOCKED`. |
| Parent makes hotfix after worker exit                   | `parent-hotfix` records event, invalidates receipts, finalizes report.                                                   |
| Parent forgets to record hotfix                         | Batch verifier detects unrecorded post-worker commits touching task paths and fails.                                     |
| Two workers append ledger event simultaneously          | Append API serializes through lock and allocates seq atomically.                                                         |
| Worker stages another task’s files                      | Staged guard fails before commit.                                                                                        |
| Raw NUL appears in source                               | Staged hygiene guard fails; batch verifier also scans final tree.                                                        |
| Receipt exists for old worker commit after parent patch | Invalid unless carried forward to final commit with proof.                                                               |
| Worker heartbeat expires                                | Task becomes `STALE`; scheduler withholds successors until resolved.                                                     |
| Ledger lock becomes stale                               | Explicit lock recovery command records a recovery event.                                                                 |
| Report finalized before latest parent patch             | Report becomes stale; batch verifier fails completion.                                                                   |

---

# 13. CLI/API surface

## Dispatch

```bash
node scripts/task-runner-dispatch.mjs \
  --manifest .missiond/tasks/wave30/manifest.lisp \
  --schedule ready-queue-v2 \
  --max-parallel 3 \
  --json
```

## Claim

```bash
node scripts/task-runner-claim.mjs \
  --wave wave30 \
  --task wave30-03 \
  --worker-id worker-a \
  --json
```

## Heartbeat

```bash
node scripts/task-runner-append-event.mjs \
  --wave wave30 \
  --task wave30-03 \
  --kind heartbeat \
  --actor-role worker \
  --actor-id worker-a \
  --payload '(:message "implementation complete; running local checks")' \
  --json
```

## Worker handoff

```bash
node scripts/task-runner-worker-handoff.mjs \
  --wave wave30 \
  --task wave30-03 \
  --commit HEAD \
  --draft-report .missiond/tasks/wave30/reports/drafts/wave30-03.report.lisp \
  --json
```

## Parent hotfix

```bash
node scripts/task-runner-parent-hotfix.mjs \
  --wave wave30 \
  --task wave30-03 \
  --base d36de8040bf0 \
  --commit d842b1d9f4aa \
  --reason "Fix lineage fields after parent patch." \
  --finalize \
  --json
```

## Verification

```bash
node scripts/task-runner-verify.mjs \
  --wave wave30 \
  --task wave30-03 \
  --tier smoke \
  --commit d842b1d9f4aa \
  --write-receipt \
  --json
```

## Finalization

```bash
node scripts/task-runner-finalize-report.mjs \
  --wave wave30 \
  --task wave30-03 \
  --json
```

## Batch verification

```bash
node scripts/verify-task-runner-batch.mjs \
  --manifest .missiond/tasks/wave30/manifest.lisp \
  --check-lifecycle \
  --check-lineage \
  --check-receipts \
  --check-source-hygiene \
  --json
```

---

# 14. Migration plan

## Wave30-01: Parent-hotfix finalizer

Implement first because it fixes the highest-risk correctness issue.

### Build

* `report-contract-v2`
* `task-runner-finalize-report.mjs`
* `task-runner-parent-hotfix.mjs`
* report stale/finalized state
* batch verifier lineage checks

### Preserve compatibility

Existing Wave29 reports remain valid under v1. New Wave30 finalized reports use v2. The checker accepts v1 for historical waves and requires v2 only for Wave30+.

### Tests

```text
worker commit only -> final report commit_hash == final_commit_hash
worker commit + parent hotfix -> report updated to parent commit
multiple parent hotfixes -> ordered parent_patches list
parent hotfix after worker exit -> no amend required
report finalized before parent hotfix -> batch verifier fails
unrecorded post-worker commit touching task paths -> batch verifier fails
```

### Invariants

```text
finalized report commit_hash == final_commit_hash
complete task verified_commit_hash == final_commit_hash
parent_patches matches parent_hotfix events
```

---

## Wave30-02: Atomic event log and projections

### Build

* `task-ledger-event-v1`
* `task-runner-append-event.mjs`
* event index with atomic seq allocation
* event lock/recovery
* shared-memory/session-trace projections
* `check-ledger-events.mjs`

### Preserve compatibility

Existing shared-memory and session-trace stay readable. New events write projections so old tooling can still inspect familiar files.

### Tests

```text
two append processes concurrently -> seq 1,2 with no duplicate
ten concurrent heartbeats -> gapless seq
same idempotency key retried -> returns existing event
manual edit to projection -> batch verifier detects drift
stale lock recovery -> recovery event recorded
```

### Invariants

```text
workers never edit shared-memory/session-trace directly
events are canonical
projections are reproducible
```

---

## Wave30-03: Staged source hygiene and scope guard

### Build

* `check-staged-source-hygiene.mjs`
* NUL-byte detection for staged source
* binary allowlist support
* staged scope guard integration
* `git diff --cached --check` integration
* final tree NUL scan in batch verifier

### Tests

```text
raw NUL in .mjs staged file -> reject
raw NUL in .lisp staged file -> reject
declared binary fixture -> allow
undeclared binary file -> reject
file outside write_scope -> reject
must_not_touch path -> reject
peer task staged file -> reject
```

### Invariants

```text
no raw binary source
no accidental peer staging
no staged must_not_touch paths
```

---

## Wave30-04: Hard deps, soft refs, and ready-queue v2

### Build

* manifest v2 fields:

  * `:depends_on`
  * `:context_refs`
  * `:artifact_requires`
  * `:write_scope`
  * `:must_not_touch`
  * `:runner_limits`
* ready-queue v2 planner
* critical-path priority
* lease model
* stale heartbeat handling

### Preserve compatibility

Default planner remains group-barrier unless explicitly invoked:

```bash
--schedule ready-queue-v2
```

### Tests

```text
hard dep incomplete -> blocked
soft context ref incomplete -> task still ready
artifact required for verification only -> implementation may dispatch
write-scope overlap -> not parallel
read-only soft ref -> never blocks
critical-path task wins deterministic tie
stale heartbeat -> successors withheld
```

### Invariants

```text
only hard deps block dispatch
soft refs influence brief/priority only
write-scope leases prevent concurrent conflicting writes
```

---

## Wave30-05: Receipt v2 and cross-layer smoke

### Build

* verification receipt v2
* receipt carry-forward mode
* receipt invalidation engine
* final report receipt alignment
* end-to-end Wave30 smoke covering:

  * prepare
  * dispatch
  * append events
  * worker handoff
  * parent hotfix
  * receipt invalidation
  * finalization
  * archive gate

### Tests

```text
local receipt cannot satisfy smoke
wrong commit invalid
command drift invalid
file footprint mismatch invalid
non-zero receipt invalid
parent patch touching footprint invalidates receipt
parent patch outside footprint creates carry-forward receipt
report finalized after receipt touching report -> receipt invalidated
complete task without final-commit receipt -> batch verifier fails
```

### Invariants

```text
complete tasks have valid receipts for final commit
carry-forward receipts explicitly cite evidence receipts
receipt reuse is conservative and explainable
```

---

# 15. Tradeoffs and risks

## Tradeoff: event log adds machinery

The event log introduces a new canonical lifecycle artifact. That is extra complexity, but it removes the current ambiguity around shared-memory, session-trace, report state, and git lineage.

## Tradeoff: locks serialize ledger appends

Atomic append means ledger writes are serialized. This is acceptable because ledger events are small and short-lived. The expensive work remains parallel.

## Tradeoff: report finalizer changes worker expectations

Workers may feel like they “wrote the report,” but in Wave30 they write a draft. This is the right boundary: workers describe what they did; the orchestrator records what finally happened.

## Risk: parent bypasses protocol

If a parent manually commits and does not run `parent-hotfix`, the system cannot prevent that at commit time unless all commits are wrapped. The mitigation is batch verification that detects unrecorded post-worker commits touching task paths.

## Risk: receipt reuse becomes too conservative

The safest invalidation rules may rerun more verification than strictly necessary. That is preferable to false reuse. Start conservative, then add declared command footprints and carry-forward receipts once tests prove safety.

## Risk: write scopes may be too broad

Broad write scopes reduce parallelism. Wave30 should report why a task was blocked by overlap so future contracts can narrow scopes.

## Risk: shared worktree still has index hazards

The staged guard and `git_index` token reduce risk, but per-worker git worktrees would be cleaner long-term. Wave30 can stay additive with shared worktree controls; Wave31 can consider isolated worktrees if collisions remain costly.

---

# 16. Recommended final shape

The clean Wave30 target is:

```text
contracts/manifests define intended work
event log records lifecycle truth
workers produce code + draft reports
parent records hotfixes as first-class events
verifier writes receipts for final commits
finalizer writes authoritative reports
archive consumes only finalized verified reports
```

The single most important invariant is:

```text
No task is complete unless its finalized report, final commit, parent patches, and verification receipts all agree.
```

That invariant directly addresses the Wave29-03 drift case while preserving the throughput gains from Wave28 and Wave29.
