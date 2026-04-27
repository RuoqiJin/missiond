;; Wave 23 / Task 03 — Task run verifier trace integration v1.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave23/wave23-03-task-run-verifier-trace-v1.lisp

(report wave23-03-task-run-verifier-trace-v1
  :schema "missiond.report-contract.v1"
  :task_id "wave23-03-task-run-verifier-trace-v1"
  :status done
  :commit_hash "5ad29004f947"
  :files_changed
    ["scripts/verify-task-run.mjs"]

  :acceptance_results
    [(:command "node scripts/verify-task-run.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "task-run verify fixtures OK (19 fixtures, 15 helper cases). 12 prior fixtures unchanged (all-green / report task_id mismatch / report commit_hash mismatch / full-sha match / message mismatch / write-scope deviation / must-not-touch hit / shared-memory missing completion / shared-memory completion for other task / --allow-missing-memory skip / missing-memory blocked / report-status-not-done warning). 7 new wave23-03 trace fixtures: (1) trace pass with matching completion event + matching :commit_hash via --require-trace, (2) trace fail when only a :kind=failure event is present for the task (verifier rejects since :kind=complete is the only acceptable success proof), (3) trace fail when both report and trace carry :commit_hash but they disagree (cross-check fires via new commitHashesAgree helper), (4) trace fail when --trace points to malformed source (loader caught traceStatus='malformed'), (5) trace pass when --trace absent and --require-trace not set (silent skip), (6) trace fail when --trace absent and --require-trace set (hard error 'session-trace required but not provided'), (7) trace pass with warning when only one side carries :commit_hash (cross-check skipped). Helper cases: 7 commitHashMatches (existing) + 8 commitHashesAgree (new symmetric prefix matcher: equality, short-prefix-of-long, long-contains-short, mismatch, <7 hex chars, empty strings either side, non-hex tokens).")
     (:command "node scripts/check-session-trace.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "session-trace fixtures OK (22 cases, 18 categories) — byte-identical to wave23-06's baseline. The verifier reuses parseTraceEvents/SCHEMA/KIND_VALUES/ENTRY_HEAD via named imports rather than touching the checker. The CLI surface, argv parsing, dry-fixture set, error messages, and exit-code paths of check-session-trace.mjs are all unchanged.")
     (:command "node scripts/check-task-contract.mjs --all"
      :exit_code 0
      :ok true
      :notes "task-contract check OK (57 tasks). The wave23-03 contract validates and the post-commit verifier cross-checked commit 5ad29004f947 against it (subject prefix `feat(tasks): verify task runs with session trace` + changed_files ⊆ write-scope + must-not-touch ∩ ∅).")
     (:command "git diff --check -- scripts/verify-task-run.mjs scripts/check-session-trace.mjs scripts/check-task-memory.mjs scripts/check-task-report.mjs scripts/lib/missiond_lisp.mjs"
      :exit_code 0
      :ok true
      :notes "git diff --check clean across all 5 in-scope paths. Only scripts/verify-task-run.mjs was modified (+461/-3); the other 4 paths are in :write-scope but were intentionally left unchanged because the necessary helpers were already exported by wave23-06 (parseTraceEvents/SCHEMA/KIND_VALUES) and the existing fixtures of those scripts must keep passing byte-identically.")]

  :scope_deviations []

  :time_sinks
    ["Reading wave23-01/02/04/06 ledger entries to confirm what was already exported and what would conflict — high payoff because parseTraceEvents was already exported by wave23-06 (e7ab99d), saving re-implementation."
     "Designing the kind decision: explicit comment in script prose noting :kind=complete is the only acceptable proof and :kind=failure is intentionally rejected (would otherwise be a footgun for future maintainers reading the code)."]

  :major_decisions
    [(:decision "Reuse imports rather than re-edit check-session-trace.mjs / check-task-report.mjs / check-task-memory.mjs / scripts/lib/missiond_lisp.mjs."
      :rationale "All four are in :write-scope but wave23-02 (16 fixtures) and wave23-06 (22 fixtures) just landed there. Touching them risks regressing fixtures that must still pass byte-identically. The only helpers needed (parseTraceEvents, KIND_VALUES, SCHEMA, ENTRY_HEAD) were already exported in commit e7ab99d. Importing is strictly less invasive than re-editing.")
     (:decision "Accept ONLY :kind=complete as proof of a successful run; reject :kind=failure."
      :rationale "A failure event is itself a fact recorded in the trace, but it is the opposite of a completion proof — accepting it would make the verifier rubber-stamp aborted runs. Documented inline near loadTrace so the kind decision survives future refactors. Failure events are still parsed and counted (the diagnostic message includes the failure count when no completion event is found) — they are visible signals, just not success proofs.")
     (:decision "Asymmetric :commit_hash → warn-and-pass; symmetric mismatch → hard fail."
      :rationale "The trace's :commit_hash is optional per the session-trace v1 schema, and the report-vs-git commit_hash check (existing) is the source of truth. Failing on one-sided absence would force every workstation that does not yet emit :commit_hash on its complete event to opt out of trace verification entirely. Failing on disagreement is non-negotiable: it is a real provenance defect.")
     (:decision "Add commitHashesAgree as a separate helper from commitHashMatches."
      :rationale "commitHashMatches is asymmetric (reported vs full git sha) — the report side might be a 7-char prefix and the git side is always the full 40-char sha. commitHashesAgree is symmetric: either side might be short or long. Conflating them would either weaken the report-vs-git check or strengthen the trace-vs-report check past what the schema permits.")
     (:decision "loadTraceFromSource mirrors parseTraceEvents shape rather than calling it."
      :rationale "parseTraceEvents takes a file path (uses readLispFile internally) and we need an in-memory parser for fixtures. Adding a second exported function to check-session-trace.mjs would mean editing it (see decision #1). Re-using parseLisp from missiond_lisp.mjs and mirroring the shape locally costs ~25 lines and keeps check-session-trace.mjs untouched. The mirror returns the same structured shape so verifyRun does not need to know which loader was used.")]

  :unexpected_work
    [(:summary "Two parallel agents (wave23-04 / wave23-05) had unstaged crates/** modifications in the worktree."
      :impact "None — never staged or committed any of those paths. The scoped pre-commit guard caught them as out-of-scope, and `git status` after the commit shows them still unstaged for those agents to claim.")]

  :blockers []

  :trace_refs
    ["wave23-trace-bootstrap-001"]

  :notes
    "Verifier scope (deliberate):
- Read-only by construction. No new git verbs added; the FORBIDDEN_GIT_VERBS allowlist + assertReadOnlyGit export are unchanged. The only git surface is still `git rev-parse | git log | git show` via verify-task-contract.readCommit. No file writes outside stdout/stderr.
- Trace is opt-in. Without --trace the verifier behaves exactly as before; the new session_trace_completion check reports `skipped: true` with reason '--trace not provided and --require-trace not set'. This preserves backward compatibility for every caller (CI, daemon, manual).
- --require-trace is the strict gate. When set, absence of --trace OR a malformed trace OR a present-but-no-completion-event trace all become hard errors. Without it, a present-but-no-completion-event trace is STILL a hard error (the operator opted in to the trace check by passing --trace, so a missing completion is a real defect, not silent telemetry); only outright absence of --trace is silent.

New CLI flags:
  --trace <session-trace.lisp>   optional; activates checks 6 and 7
  --require-trace                promotes absence/load-failure to hard error

New verifier checks:
  Check 6 (session_trace_completion): trace must contain at least one
    (trace-event :task <id> :kind complete) entry. Diagnostic includes
    the count of :kind=failure events for the task when no completion
    is found, surfacing the asymmetry to the operator.
  Check 7 (session_trace_commit_hash): when BOTH report and the matched
    completion event carry :commit_hash, they MUST agree. Asymmetric
    case (one side empty) → cross-check skipped with a warning, run
    still passes.

New exports:
  loadTrace(file)            — wraps parseTraceEvents from check-session-trace.mjs
  loadTraceFromSource(src)   — in-memory parser for fixtures
  commitHashesAgree(a, b)    — symmetric prefix matcher (distinct from
                               existing commitHashMatches which is asymmetric)

verifyRun() now accepts five additional optional fields:
  trace, traceFile, traceStatus, traceLoadError, requireTrace.
  Default values keep all existing callers green.

Live smoke (against committed history):
- node scripts/verify-task-run.mjs --task wave23-06... --report ... --memory ... --commit e7ab99d
  → OK (no --trace, backward compatible).
- ... --require-trace → hard fail 'session-trace required but not provided' (exit 1).
- ... --trace .missiond/tasks/wave23/session-trace.lisp
  → hard fail 'no (trace-event :task wave23-06... :kind complete) entry' (exit 1; expected — the seed trace only carries the bootstrap observation, not a completion event).

Workflow protocol followed:
- Hooks preflight: aligned (core.hooksPath==.githooks); `node scripts/check-missiond-hooks.mjs --json` returned ok=true severity=ok matches=true reason=aligned.
- Shared-memory ledger appended: (claim wave23-03-claim-001 :seq 13) before edit, (completion wave23-03-completion-001 :seq 15) after commit. A parallel agent inserted (claim wave23-05-claim-001 :seq 14) between them — expected interleaving.
- Pre-commit task-scope-guard (--mode staged): 1 staged file in :write-scope, zero matches against :must-not-touch (crates/** / .missiond/v2/*.lisp / .missiond/tasks/schema/*.lisp / .missiond/tasks/wave22/**).
- MISSIOND_TASK_CONTRACT env var set on the git commit invocation; the .githooks/pre-commit hook re-ran the same guard.
- Post-commit verify: scripts/verify-task-contract.mjs cross-checked 5ad29004f947 against the contract — message prefix matches, changed_files ⊆ write-scope, must-not-touch ∩ ∅. All green.

Constraints honored:
- DID NOT touch crates/** (two parallel agents have unstaged modifications there; left them alone).
- DID NOT touch .missiond/v2/*.lisp.
- DID NOT touch .missiond/tasks/schema/*.lisp (out of scope for this task).
- DID NOT touch .missiond/tasks/wave22/**.
- DID NOT modify scripts/check-session-trace.mjs (in :write-scope but its 22-fixture suite must keep passing byte-identically; reused exports instead).
- DID NOT modify scripts/check-task-report.mjs (in :write-scope but wave23-02 just added 5 explanation fields with 6 new fixtures; touching it risks regressing the 16-fixture suite).
- DID NOT modify scripts/check-task-memory.mjs (in :write-scope but no edits needed).
- DID NOT modify scripts/lib/missiond_lisp.mjs (in :write-scope but parseLisp + readKeywordProps + nodeText + nodeToStringArray + isList + head are all already exported and sufficient).
- The verifier remains strictly read-only (no git mutations, no file writes outside stdout/stderr).
- NO git add . / git stash / git reset / git checkout / amend / push / --no-verify.

Wave23 ledger edits and this report file are intentionally out-of-scope of the commit and remain untracked / unstaged for the next wave's archive task per the established protocol.")
