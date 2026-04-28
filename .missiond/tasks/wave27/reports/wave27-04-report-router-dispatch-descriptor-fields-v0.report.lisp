;; Wave 27 / Task 04 — Report router dispatch descriptor fields v0.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave27/wave27-04-report-router-dispatch-descriptor-fields-v0.lisp

(report wave27-04-report-router-dispatch-descriptor-fields-v0
  :schema "missiond.report-contract.v1"
  :task_id "wave27-04-report-router-dispatch-descriptor-fields-v0"
  :status done
  :commit_hash "afb5ffbc3d794dcd17b29c52ebb2741bfa4c135a"
  :files_changed
    [".missiond/tasks/schema/report-contract-v1.lisp"
     "scripts/check-task-report.mjs"]

  :acceptance_results
    [(:command "node scripts/check-task-report.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "task-report fixtures OK (38 cases). 31 prior fixtures (wave19-fix-* / wave23-fix-* / wave25-fix-* / wave25-05 / wave26-fix-* / wave26-06-fix-*) stay byte-identical green; 7 new wave27-04 fixtures all pass: wave27-fix-dispatch-legacy (no descriptor fields → backward-compat), wave27-fix-dispatch-ok (all 6 fields, claudecode seed shape, eligible=false, no_execution=true), wave27-fix-dispatch-no-exec-false (cross-wave invariant: literal false rejected), wave27-fix-dispatch-no-exec-string (literal-atom invariant: \"true\" string rejected), wave27-fix-dispatch-eligible-string (literal-atom invariant: \"false\" string rejected), wave27-fix-dispatch-bad-status (closed enum rejection: status=pending), wave27-fix-dispatch-abs-path (repo-relative invariant: /etc/descriptor.lisp rejected).")
     (:command "node scripts/check-task-report.mjs --all"
      :exit_code 0
      :ok true
      :notes "task-report check OK (57 reports). Every existing real report under .missiond/tasks/**/reports/*.report.lisp parses + validates; the additive optional-field surface cannot mark a previously-valid report invalid. Confirms backward-compat across all wave19..wave27 reports.")
     (:command "node scripts/check-task-contract.mjs --all"
      :exit_code 0
      :ok true
      :notes "task-contract check OK (92 tasks) — all wave22..wave27 task contracts including this one parse and pass shape / scope / must-not-touch / acceptance / commit-policy validation. No regressions vs the wave27-01 baseline (also 92).")
     (:command "git diff --check -- .missiond/tasks/schema/report-contract-v1.lisp scripts/check-task-report.mjs"
      :exit_code 0
      :ok true
      :notes "no whitespace errors on either staged path; trailing-newline / tab-stop hygiene clean on both edited files.")
     (:command "node scripts/check-missiond-hooks.mjs --json"
      :exit_code 0
      :ok true
      :notes "preflight OK; core.hooksPath aligned to .githooks; .githooks/pre-commit exists and is executable; no install required.")
     (:command "node scripts/task-scope-guard.mjs --task .missiond/tasks/wave27/wave27-04-report-router-dispatch-descriptor-fields-v0.lisp --mode staged"
      :exit_code 0
      :ok true
      :notes "task-scope-guard staged OK: wave27-04-report-router-dispatch-descriptor-fields-v0 (2 staged file(s)) — both staged paths inside :write-scope; zero matches against :must-not-touch (crates/** .missiond/v2/** .missiond/router/** .missiond/tasks/wave27/wave27-*.lisp .missiond/claudecode/** scripts/check-router-dispatch-descriptor.mjs scripts/build-router-dispatch-descriptor.mjs scripts/recommend-task-backend.mjs scripts/evaluate-router-policy-corpus.mjs scripts/render-claudecode-task.mjs).")
     (:command "node scripts/verify-task-contract.mjs .missiond/tasks/wave27/wave27-04-report-router-dispatch-descriptor-fields-v0.lisp"
      :exit_code 0
      :ok true
      :notes "task-contract verify OK: wave27-04-report-router-dispatch-descriptor-fields-v0 against afb5ffbc3d79 — commit hash exists; commit message matches `feat(tasks): record router dispatch descriptors in reports` per contract; changed_files ⊆ write-scope (write-scope-only); changed_files ∩ must-not-touch = ∅; acceptance commands present in contract.")]

  :scope_deviations []

  :trace_refs [wave27-trace-04-start-001 wave27-trace-04-commit-001 wave27-trace-04-complete-001]

  :major_decisions
    [(:decision "Reuse wave25-02 + wave26-04 helpers exclusively for the 5 boolean / enum / path checks; introduce only ONE new helper (validateRouterDispatchBlockers)."
      :rationale "Mirrors the wave26-04 pattern (which itself reused wave25-02). Reducing helper proliferation keeps a single source of truth for each rule shape (literal-atom-true-only / literal-atom-either-polarity / closed-enum / repo-relative-path / non-empty-string-vector). The new helper is a name-only rename of validateRouterApplyBlockers because the field name ends up in error messages and we want grep-friendly diagnostics.")
     (:decision "Use validateRouterLiteralBool('true') for :router_dispatch_no_execution rather than adding a second positional helper."
      :rationale "The existing wave25-02 helper already takes the expected literal as its 5th parameter (used today for :router_dry_run_only=true and :router_applied=false); reusing it for the wave27-04 no_execution=true lock keeps the code path identical to the cross-wave invariant established in wave25-02.")
     (:decision "Place all 6 new fields as flat top-level optional report fields (no nested router_dispatch block)."
      :rationale "Mirrors how wave25-02 (7 fields) and wave26-04 (5 fields) flattened their optional surfaces. A nested block would force the checker to thread loc-tracking through an extra layer for almost no readability gain — and would break the wave23-02 prose-field reuse of readKeywordProps with start:2.")
     (:decision "Add the wave27-04 cluster comment to the optional-report-fields declaration AND repeat the full lockdown rationale in field-contract for each of the 6 new keys."
      :rationale "Future maintainers reading just the schema (without checker source) need to see why no_execution is unidirectional. Duplication is intentional — the cluster header explains the WHY once, the per-field contract pins the WHAT for each field individually.")]

  :time_sinks
    [(:label "Reading wave25-02 + wave26-04 helper definitions to confirm reuse vs new"
      :notes "5 helpers existed (validateRouterEnumField, validateRouterRepoRelativePath, validateRouterLiteralBool, validateRouterLiteralBoolEither, validateRouterApplyBlockers); validated they all do exactly what wave27-04 needs without any change. Only validateRouterDispatchBlockers added — it is structurally identical to validateRouterApplyBlockers but emits :router_dispatch_blockers in errors so diagnostics grep naturally.")
     (:label "Drafting 7 fixtures with byte-identical preservation of the prior 31"
      :notes "Used Edit (not Write) on both files. Inserted new fixtures inside the closing `]` of the fixtures array, after the wave26-06 entry. Confirmed legacy fixture exists per requirement (no descriptor fields = pass). The 5 negatives match each wave27-04 invariant 1:1.")]

  :unexpected_work
    [(:summary "Updated checker-contract :rejects list in the schema to enumerate 6 new error classes (descriptor_status enum / dispatch_backend enum / eligible literal-atom / no_execution literal-atom-true-only / dispatch_blockers shape / abs descriptor_path). Also extended the :non-goal sentence to mention router-dispatch-descriptor fields are observational alongside router-recommendation and router-readiness — keeps the schema's documentation surface in sync with the checker.")]

  :recommended_backend "claudecode"
  :router_confidence "high"
  :router_policy_path ".missiond/router/router-policy-v1.lisp"
  :router_dry_run_only true
  :router_applied false
  :router_reasons
    ["Workstation surface (Lisp schema edit + Node.js checker edit, additive only, no Rust / SQL / cargo) is the canonical claudecode beat — matches r-fresh-code-alignment-to-claudecode in router-policy-v1."
     "Strict additive backward-compat constraint required all 31 existing fixtures to stay byte-identical green; ClaudeCode is the established default for low-risk schema/checker extensions."
     "Router output is recorded for telemetry only; runtime dispatch unchanged (claudecode is the live default and remained the live default for this task)."]
  :router_trace_index_path ".missiond/router/trace-index-v1.lisp"

  :router_backend_readiness_status "current-default"
  :router_backend_runtime_allowed true
  :router_apply_eligible false
  :router_apply_blockers
    ["backend claudecode readiness_status=current-default (apply gate requires runtime-ready; current-default is NOT sufficient)"
     "explicit runtime-ready opt-in required upstream before live promotion"]
  :router_backend_registry_path ".missiond/router/router-backend-registry-v1.lisp"

  :router_dispatch_descriptor_path ".missiond/router/dispatch-descriptors/wave27-04-report-router-dispatch-descriptor-fields-v0.lisp"
  :router_dispatch_descriptor_status "absent"
  :router_dispatch_backend "claudecode"
  :router_dispatch_eligible false
  :router_dispatch_no_execution true
  :router_dispatch_blockers
    ["wave27-02 builder has not yet emitted a descriptor for this task; descriptor_status=absent records the handoff fact without claiming runtime backend execution"
     "descriptor recording NEVER asserts a runtime backend swap happened (cross-wave invariant — :router_dispatch_no_execution locked literal true)"
     "apply gate requires runtime-ready; current-default is NOT sufficient"]

  :notes
    "wave27-04 ships:
     - .missiond/tasks/schema/report-contract-v1.lisp: 6 new optional flat report fields under a wave27-04 cluster comment in optional-report-fields, full per-field declarations in field-contract, and 6 new error classes in checker-contract :rejects. wave23-02 + wave25-02 + wave26-04 declarations preserved verbatim.
     - scripts/check-task-report.mjs: 1 new closed-enum constant (ALLOWED_ROUTER_DISPATCH_STATUS, 5 values: absent | built | invalid | registry_missing | blocked); 6 new validateReport calls wired after the wave26-04 block; 1 new helper (validateRouterDispatchBlockers, structurally identical to validateRouterApplyBlockers — name divergence keeps grep-friendly diagnostics); 7 new dry-fixture cases under a wave27-04 cluster comment.

     6 new fields (all optional, flat top-level):
       :router_dispatch_descriptor_path   — repo-relative string (no leading '/' or '~', no '..') via wave25-02 validateRouterRepoRelativePath
       :router_dispatch_descriptor_status — closed enum {absent | built | invalid | registry_missing | blocked} via wave25-02 validateRouterEnumField
       :router_dispatch_backend           — closed enum (5-value router backend enum reused from wave25-02 ALLOWED_ROUTER_BACKENDS) via wave25-02 validateRouterEnumField
       :router_dispatch_eligible          — literal atom true|false (strings rejected) via wave26-04 validateRouterLiteralBoolEither
       :router_dispatch_no_execution      — literal atom true ONLY (false AND strings rejected — cross-wave invariant) via wave25-02 validateRouterLiteralBool('true')
       :router_dispatch_blockers          — vector of non-empty strings via NEW wave27-04 validateRouterDispatchBlockers

     Helpers reused (no new code paths beyond the dispatch-blockers wrapper):
       wave25-02: validateRouterEnumField, validateRouterRepoRelativePath, validateRouterLiteralBool
       wave26-04: validateRouterLiteralBoolEither

     Helpers added (1):
       wave27-04: validateRouterDispatchBlockers (structural twin of validateRouterApplyBlockers; rename only — diagnostic strings reference :router_dispatch_blockers so grep on test failures is unambiguous between the wave26-04 readiness blockers and the wave27-04 dispatch blockers).

     Fixture totals: 31 → 38 (+7). Prior 31 stay byte-identical (verified by the dry-fixture pass list output before/after — every wave19/wave23/wave25/wave25-05/wave26/wave26-06 fixture name still prints `fixture OK:`). New 7:
       1. wave27-fix-dispatch-legacy                — no descriptor fields → must pass (backward compat)
       2. wave27-fix-dispatch-ok                    — all 6 fields populated, eligible=false, no_execution=true → must pass
       3. wave27-fix-dispatch-no-exec-false         — :router_dispatch_no_execution false → must fail (cross-wave invariant)
       4. wave27-fix-dispatch-no-exec-string        — :router_dispatch_no_execution \"true\" string → must fail (literal-atom invariant)
       5. wave27-fix-dispatch-eligible-string       — :router_dispatch_eligible \"false\" string → must fail (literal-atom invariant)
       6. wave27-fix-dispatch-bad-status            — :router_dispatch_descriptor_status \"pending\" → must fail (closed enum)
       7. wave27-fix-dispatch-abs-path              — :router_dispatch_descriptor_path \"/etc/descriptor.lisp\" → must fail (repo-relative invariant)

     Cross-wave invariant re-pinned: :router_dispatch_no_execution must be the literal atom true. The literal atom false AND any quoted-string form are both rejected by the wave25-02 validateRouterLiteralBool('true') helper. Fixtures #3 and #4 prove both rejection paths fire. The descriptor recording layer is locked no-execution by the schema AND by the checker.

     Pre-commit pipeline: check-task-report.mjs --dry-fixture (exit=0, 38/38) → check-task-report.mjs --all (exit=0, 57 reports) → check-task-contract.mjs --all (exit=0, 92 tasks) → git diff --check (exit=0) → check-missiond-hooks.mjs --json (preflight aligned) → git add (2 paths) → task-scope-guard.mjs --mode staged (OK, 2 staged) → MISSIOND_TASK_CONTRACT=... git commit -m \"feat(tasks): record router dispatch descriptors in reports\" (commit afb5ffbc3d79) → verify-task-contract.mjs (OK against afb5ffbc3d79). All append-only ledger updates: shared-memory wave27-04-claim-001 (seq 7) before edits + wave27-04-completion-001 (seq 9) after verify; session-trace wave27-trace-04-start-001 (seq 9) before reading background + wave27-trace-04-commit-001 (seq 11, with commit_hash) + wave27-trace-04-complete-001 (seq 12). Both ledgers re-validated after each append.

     Constraints honored: NO Rust / SQL / Cargo edits. Did not touch crates/**, .missiond/v2/**, .missiond/router/**, any wave27-*.lisp other than session-trace + shared-memory (both are session-trace-writable / claim-allowed and explicitly NOT in :must-not-touch), .missiond/claudecode/**, scripts/check-router-dispatch-descriptor.mjs, scripts/build-router-dispatch-descriptor.mjs, scripts/recommend-task-backend.mjs, scripts/evaluate-router-policy-corpus.mjs, scripts/render-claudecode-task.mjs. Did not git add . / git push / --no-verify / --amend / --force.")
