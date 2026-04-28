# MissionD Wave30 Orchestrator Architecture Research Pack

Prepared for GPT Pro architectural review.

## How To Use This Pack

Paste this whole file into GPT Pro and ask for a proposed architecture. The goal is not another narrow patch. The goal is a coherent orchestrator/task-runner design that absorbs the lessons from Wave28 and Wave29:

- ready-queue scheduling
- atomic coordination ledgers
- parent hotfix lineage
- verification receipts
- context atlas and pattern-card routing
- artifact lifecycle for reports/briefs/skeletons/archive/backfill

## Copyable GPT Pro Prompt

```text
You are reviewing MissionD's multi-agent task orchestration system. We have two recent waves of empirical data (Wave28 and Wave29) showing major throughput gains from thin briefs, shared preambles, ready-queue scheduling, context atlases, pattern cards, verification receipts, and commit lineage fields. We also found new failure modes: parent hotfix report drift, shared ledger write contention, raw NUL bytes from large edits, and ambiguity between hard dependencies and soft context references.

Please propose an elegant, complete architecture for the next iteration. Do not suggest another collection of isolated patches. Design the core state model, ownership boundaries, event log / ledger API, commit lineage lifecycle, scheduler semantics, artifact lifecycle, verification receipt semantics, and migration plan.

Required output:
1. A concise diagnosis of the current system.
2. A proposed target architecture with named components and responsibilities.
3. A formal state/event model for tasks, commits, reports, parent patches, receipts, and ledger events.
4. Scheduler semantics: hard deps vs soft context refs, ready-queue, critical path priority, write-scope overlap constraints, and backpressure.
5. A parent-hotfix protocol that guarantees reports stay aligned after parent commits.
6. An atomic ledger append protocol that removes read-max-seq/edit races.
7. Verification receipt rules: when evidence can be reused, invalidated, or escalated.
8. Source hygiene rules: NUL-byte hook, staged scope guard, no raw binary source.
9. A migration plan in 3-5 implementation waves, with tests and invariants for each.
10. Explicit tradeoffs and risks.

Constraints:
- Existing system uses Lisp task contracts/reports/manifests as machine-readable SSOT.
- Existing worker-facing briefs are generated Markdown, not authoritative.
- Node checkers/CLIs must remain deterministic and no network/no LLM/no git mutation unless explicitly named as a git verifier.
- Archive/backfill/index are orchestrator-owned and must not become worker tasks.
- Parent orchestrator may make hotfix commits after a worker exits; the architecture must handle this without amending worker commits.
- Shared-memory/session-trace are append-only coordination logs today, but direct worker editing causes sequence races.
- Prefer additive migrations that preserve Wave28/Wave29 behavior until replacements are verified.
```

## Executive Summary

Wave28 proved that thin briefs, shared preamble, verification tiers, and productive-only dispatch cut wall-clock from roughly 3.5 hours to 52m11s.

Wave29 kept roughly the same wall-clock (51m49s) while doing more work: 7 productive tasks instead of 6, around 9700+ net code lines, 10 commits, six new mechanisms, and true 3-way plus 2-way parallelism. The optimization has shifted from "single worker faster" to "system throughput higher while parallelism remains controlled."

The remaining pain is no longer prompt size. It is architecture:

- parent hotfixes can happen after a worker exits, leaving reports stale unless the parent explicitly rewrites lineage fields
- shared ledger append is not atomic, so parallel workers fight over `:seq`
- ready-queue exists but hard dependencies and soft context references are still conflated by the dispatcher
- NUL-byte source corruption has appeared twice after large edits
- reports and ledger entries are still largely worker-written rather than orchestrator-owned state transitions

## Current System Pieces

### Authoritative Inputs

- Task contracts: `.missiond/tasks/wave*/wave*-*.lisp`
- Task runner manifest: `.missiond/tasks/wave29/manifest.lisp`
- Reports: `.missiond/tasks/wave29/reports/*.report.lisp`
- Shared memory: `.missiond/tasks/wave29/shared-memory.lisp`
- Session trace: `.missiond/tasks/wave29/session-trace.lisp`
- Generated briefs: `.missiond/claudecode/wave29-*.md`

### Main Scripts

- `scripts/check-task-contract.mjs`
- `scripts/render-claudecode-task.mjs`
- `scripts/render-wave-briefs.mjs`
- `scripts/prepare-task-runner-wave.mjs`
- `scripts/check-task-runner-manifest.mjs`
- `scripts/plan-task-runner.mjs`
- `scripts/check-task-report.mjs`
- `scripts/verify-task-run.mjs`
- `scripts/verify-task-runner-batch.mjs`
- `scripts/check-context-atlas.mjs`
- `scripts/check-pattern-card.mjs`
- `scripts/check-verification-receipt.mjs`

### Durable Schemas Added Recently

- `.missiond/tasks/schema/task-runner-manifest-v1.lisp`
- `.missiond/tasks/schema/context-atlas-v1.lisp`
- `.missiond/tasks/schema/pattern-card-v1.lisp`
- `.missiond/tasks/schema/verification-receipt-v1.lisp`
- `.missiond/tasks/schema/report-contract-v1.lisp` with commit-lineage fields

### Orchestration Concepts

- `productive_only=true`: archive/backfill/index/lisp-backfill are not worker tasks.
- `verification_tier`: `local | smoke | full`
- `dispatch_group`: grouping for parallel eligibility and overlap checks.
- `estimated_minutes` and `heartbeat_minutes`: scheduling/monitoring metadata.
- `context_atlas_path` and `pattern_card_path`: structured navigation inputs.
- `ready_queue`: additive planner output behind `--schedule ready-queue`.
- verification receipts: reusable evidence for commands against commit/tier/files.
- parent patches: report lineage records for post-worker hotfix commits.

## Wave28 Baseline

### What Changed

Wave28 introduced:

- thin brief + shared preamble
- task-runner manifest
- batch brief renderer
- local/smoke/full verification tiers
- batch verifier
- daemon dry-run surface for task-runner manifest
- cross-layer smoke

### Measured Result

- total wall-clock: 52m11s
- 6 productive tasks
- about 383 tool calls
- around 7820 net code lines
- 6 commits plus one parent hotfix
- daemon tests: 1667 -> 1681 after Wave28/29 sequence context

### Key Wins

- No archive/backfill/index worker tasks.
- Thin briefs removed repeated boilerplate.
- Verification tiers stopped every worker from running full cargo.
- Batch verifier started joining manifest, reports, shared-memory, and commits.
- Cross-layer smoke caught real DAG critical-path behavior.

### Key Remaining Issue From Wave28

Wave28-02 worker commit `954116e` got parent hotfix commit `302330a`. The report initially pointed at the worker commit, while HEAD/final bytes included the parent fix. This proved that "verify against HEAD" and "worker report commit hash" are insufficient under parent hotfixes.

## Wave29 Additions

Wave29 added:

- context atlas schema/checker and real atlas
- pattern-card schema/checker and seed pattern cards
- runner wave prep CLI
- commit-lineage v1 hardening
- verification receipt schema/checker + batch integration
- ready-queue planner
- 8-layer runner-efficiency smoke

### Wave29 Timing

| Task | Wall | Tools | Commits | Notes |
|---|---:|---:|---:|---|
| 29-01 context atlas | 10m44s | 52 | 1 | schema + checker + real atlas |
| 29-02 pattern card | 15m36s | 77 | 1 | schema + checker + 5 seed cards |
| 29-03 runner prep | 10m22s | 65 | 2 | parent hotfix after worker exit; lineage drift |
| 29-04 hotfix lineage | 14m14s | 100 | 2 | self-demo of lineage |
| 29-05 verification receipt | 19m15s | 105 | 1 | receipt schema + batch integration |
| 29-06 ready queue | 18m03s | 122 | 2 | self-fix lineage complete |
| 29-07 efficiency smoke | 12m54s | 96 | 1 | 8-layer smoke, no hotfix |

End-to-end wall-clock:

- 14:09:46 -> 15:01:35 = 51m49s
- serial equivalent = 101m08s
- total acceleration = about 1.95x

Wave29 did more than Wave28 in roughly the same time:

- 7 productive tasks vs 6
- about 617 tool calls vs 383
- about 9700+ net code lines vs about 7820
- 10 commits vs 6/7
- six new mechanisms
- real 3-way and 2-way parallel execution

## Parallelism Observations

### Group A

Wave29-01, Wave29-02, and Wave29-04 launched in a 3-way ready queue.

- wall-clock: 16m10s
- serial equivalent: 40m34s
- speedup: 2.51x

Observed coordination cost:

- workers repeatedly reread ledger max seq
- workers checked `git status` before staging
- one worker used `verify-task-contract --commit=<hash>` to avoid HEAD drift

Net result: roughly 6 minutes of coordination friction bought about 24 minutes of wall-clock savings.

### Group B

Wave29-05 and Wave29-06 ran in parallel.

- wall-clock: 19m15s
- serial equivalent: 37m18s
- speedup: 1.94x

This was the cleanest parallel group. However, ledger contention became visible:

- 13 ledger rereads
- 3 retry loops due to concurrent edits
- workers had to unstage each other's accidentally staged files

### Ready Queue Output On Wave29 Manifest

`scripts/plan-task-runner.mjs --manifest .missiond/tasks/wave29/manifest.lisp --schedule ready-queue --json` produced:

- existing group-barrier batches: A -> B -> C
- `ready_queue.order`:
  - 29-01
  - 29-02
  - 29-04
  - 29-05
  - 29-06
  - 29-03
  - 29-07
- `aggregate_idle_window_savings_minutes`: 5
- `wave_duration_minutes`: 125
- `wave_duration_savings_minutes`: 0

The output is additive. Default schedule remains group-barrier for backward compatibility.

## Problems To Solve Architecturally

### 1. Parent Hotfix Report Drift

Observed in Wave29-03:

- worker commit: `d36de80`
- parent hotfix: `d842b1d`
- report still had `:commit_hash "d36de8040bf0"`
- report lacked `:agent_commit_hash`, `:final_commit_hash`, `:verified_commit_hash`, and `:parent_patches`

Why this happens:

- worker writes report before parent hotfix exists
- parent fixes after worker exits
- lineage schema exists, but no actor is responsible for updating the report after parent hotfix

Counterexamples where it worked:

- Wave29-04 self-created a second commit and updated its own report.
- Wave29-06 self-created a lint-cleanup hotfix and filled lineage fields.

Conclusion:

Lineage v1 works when the same agent makes both commits. It fails when the parent orchestrator patches after worker exit.

Architectural need:

- parent hotfix must be a first-class state transition
- parent must either update report itself or emit an event that a deterministic finalizer consumes
- report finalization should not depend on the worker being alive
- no `git commit --amend` requirement

### 2. Ledger Append Races

Current model:

- workers read `shared-memory.lisp` or `session-trace.lisp`
- compute max `:seq`
- use file edit to append before final `)`

Observed issue:

- concurrent workers collide
- files keep changing during Edit
- workers retry and recalculate seq

Architectural need:

- append-only ledger writer API
- central sequence allocation
- ideally one command such as:
  - `node scripts/append-ledger-event.mjs --ledger ... --kind claim --task ... --json`
  - or daemon/MissionD-owned append endpoint
- worker should not manually edit final paren

### 3. Hard Dependencies vs Soft Context References

Wave29-03 started later than theoretically necessary.

Observed:

- Wave29-01 completed at 14:20:30.
- Wave29-03 started at 14:27:22.
- It likely waited for Wave29-02 and Wave29-04 because they were useful context or group peers, not strictly necessary for all parts.

Manifest says Wave29-03 depends on Wave29-01 and Wave29-02. But the analysis suggests some dependencies may be soft context references rather than hard execution blockers.

Architectural need:

- distinguish hard deps from soft refs:
  - `depends_on`: cannot run until complete
  - `context_refs`: useful to read if available, but not blocking
  - `artifact_requires`: only blocks a named action, not whole task
- scheduler should release work on hard dependency satisfaction only

### 4. Source NUL Bytes

Observed twice:

- Wave28-02 / commit `37d7e32` fixed raw NUL in `scripts/plan-task-runner.mjs`
- Wave29-06 large Edit inserted raw NUL again

Impact:

- `rg`/grep treats source as binary
- navigation quality drops
- workers waste time diagnosing tool behavior

Architectural need:

- pre-commit hook or staged guard for raw NUL in text/source files
- should run alongside `git diff --check`
- should apply to staged paths only

### 5. Artifact Ownership Is Split

Current artifact ownership:

- contracts/manifests generated by orchestrator
- briefs generated by renderer
- reports written by workers
- parent hotfixes created by parent
- ledgers edited by workers and parent
- archive/backfill owned by orchestrator

Problem:

Final truth is distributed across several actors and several file types. This creates ambiguity when parent edits after worker exits.

Architectural need:

- declare the canonical lifecycle of a task:
  - planned
  - dispatched
  - claimed
  - preamble-read
  - worker-commit
  - parent-patch
  - verified
  - report-finalized
  - archived
- each transition has exactly one owner
- each transition has deterministic verifier checks

## Non-Negotiable Existing Invariants

Keep these unless GPT Pro gives a strong reason and migration path:

- Lisp contracts/reports/manifests are machine SSOT.
- Markdown briefs are generated views, never authoritative.
- Checkers are deterministic and read-only.
- No network, no LLM, no git mutation in checkers.
- Verifiers may read git but must not mutate git.
- Archive/backfill/index/lisp-backfill are orchestrator-owned, never worker tasks.
- Scope guard must block staged files outside task `:write-scope`.
- `must-not-touch` must remain hard.
- Productive-only remains default.
- Verification tiers remain local/smoke/full.
- Existing default group-barrier plan output remains byte-compatible unless explicitly using `--schedule ready-queue`.

## Desired Target Design Questions

Ask GPT Pro to answer these concretely.

### Component Model

What components should exist?

Possible components to evaluate:

- `task-runner prepare`
- `task-runner dispatch`
- `task-runner append-event`
- `task-runner finalize`
- `task-runner parent-hotfix`
- `task-runner verify`
- `task-runner archive`

Question:

Should this stay as Node CLIs over Lisp files, move into the daemon, or split between orchestrator-owned Node CLIs and daemon dry-run surfaces?

### Event Model

What is the minimal event schema?

Candidate event kinds:

- `planned`
- `brief-rendered`
- `dispatched`
- `claimed`
- `read-preamble`
- `heartbeat`
- `worker-commit`
- `parent-hotfix`
- `verification-receipt`
- `report-written`
- `report-finalized`
- `blocked`
- `complete`
- `archived`

Question:

Should shared-memory and session-trace stay separate, or should they be projections of a single append-only event log?

### Commit Lineage Model

Current fields:

- `:commit_hash`
- `:agent_commit_hash`
- `:final_commit_hash`
- `:verified_commit_hash`
- `:parent_patches`

Question:

What is the precise meaning of each field? Should `:commit_hash` always be final commit, or should it remain worker commit with a separate final field? Wave29-04/06 used `:commit_hash` as final. Wave29-03 drifted because parent did not update it.

Need a state machine that covers:

- worker commit only
- worker commit + worker self-fix
- worker commit + parent hotfix after worker exits
- multiple parent hotfixes
- verification after any of the above

### Scheduler Model

Current model:

- manifest has `depends_on`, `dispatch_group`, `estimated_minutes`, `heartbeat_minutes`, `write_scope`
- ready-queue output is additive under `--schedule ready-queue`

Question:

How should scheduler distinguish:

- hard dependencies
- soft context references
- artifact dependencies
- read-only soft ordering
- write-scope conflicts

Need:

- deterministic priority
- critical path handling
- backpressure/concurrency limit
- stale/blocked task handling
- retry semantics

### Verification Receipts

Current idea:

- receipt tied to commit, command, tier, files/paths, exit code, time
- reusable only under conservative match

Question:

What is the right invalidation model?

Examples:

- local receipt cannot satisfy smoke/full
- wrong commit invalid
- command string drift invalid
- file set mismatch invalid
- non-zero exit invalid
- parent hotfix invalidates worker receipt unless file set unaffected?

### Atomic Ledger Append

Question:

What is the simplest append protocol?

Options:

- Node CLI with file lock
- daemon endpoint
- append-only JSONL plus generated Lisp projection
- keep Lisp as primary and use lock file
- Git-based event commit log

Need:

- concurrent safe sequence allocation
- no manual Edit before final paren
- easy validation
- deterministic archival

### Source Hygiene

Need a robust policy for:

- raw NUL in staged files
- git diff whitespace
- generated files
- binary files
- accidental staging from peer workers

Question:

Should staged guard own this, or a separate hook?

## Current Evidence References

Commits:

- `4717713` Wave28 cross-layer smoke
- `3cd1c9b` Wave28 v2 backfill
- `b9a09ac` context atlas / pattern-card metadata and lineage prep
- `4a62eda` Wave28 archive
- `37d7e32` raw NUL cleanup in planner source
- `1a5b01f` Wave29 context atlas
- `57fdc88` Wave29 pattern cards
- `c037c63` Wave29 lineage hardening worker commit
- `d7e314a` Wave29 lineage self-demo hotfix
- `d36de80` Wave29 runner prep worker commit
- `d842b1d` Wave29 runner prep parent hotfix, report drift case
- `ed7940f` Wave29 verification receipts
- `15c5267` Wave29 ready-queue worker commit
- `1951aaa` Wave29 ready-queue self-fix lineage complete
- `08bf1a6` Wave29 runner-efficiency smoke

Important files:

- `.missiond/tasks/wave29/manifest.lisp`
- `.missiond/tasks/wave29/reports/wave29-03-runner-wave-prep-v0.report.lisp`
- `.missiond/tasks/wave29/reports/wave29-06-ready-queue-planner-v0.report.lisp`
- `scripts/prepare-task-runner-wave.mjs`
- `scripts/plan-task-runner.mjs`
- `scripts/verify-task-runner-batch.mjs`
- `scripts/check-task-report.mjs`
- `scripts/check-verification-receipt.mjs`
- `scripts/check-context-atlas.mjs`
- `scripts/check-pattern-card.mjs`

## Recommended Research Deliverable Shape

Ask GPT Pro for something implementable, not just conceptual:

1. Proposed data model with sample Lisp or event records.
2. CLI/API surface with command examples.
3. Ownership table: parent vs worker vs verifier vs archiver.
4. Invariants table.
5. Failure-handling table.
6. Migration waves:
   - Wave30-01: parent hotfix finalizer
   - Wave30-02: atomic ledger append
   - Wave30-03: staged NUL/source hygiene hook
   - Wave30-04: hard deps vs soft refs in manifest/planner
   - Wave30-05: final cross-layer smoke
7. Test plan:
   - parent-hotfix after worker exit updates report
   - multiple parent patches
   - two workers appending ledger concurrently
   - raw NUL staged file rejected
   - ready-queue releases hard-dep-ready tasks despite soft refs
   - receipt reuse is invalidated correctly

## Current Working Hypothesis

A coherent target architecture probably needs a single orchestrator-owned task-run lifecycle:

```text
manifest -> prepare -> dispatch -> claim -> worker commit -> optional parent patch(es) -> verify -> finalize report -> batch verify -> archive
```

Workers should still implement code, run local acceptance, and write draft reports. But final truth should be parent/orchestrator-owned:

- parent allocates ledger sequences
- parent records parent patches
- parent finalizes report commit lineage
- verifier consumes final report, not worker draft
- archive consumes finalized reports only

The likely architecture boundary is:

- workers may append intent-like facts via an append API, not direct file edits
- workers may create a draft report
- orchestrator owns final report mutation after any parent patch
- scheduler owns task release based on hard dependencies only
- soft context references affect brief content and priority, not blocking

GPT Pro should challenge or refine this hypothesis.
