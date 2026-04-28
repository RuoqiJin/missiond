;; Wave 26 task report.
;; Schema: missiond.report-contract.v1

(report wave26-04-report-router-readiness-fields-v0
  :schema "missiond.report-contract.v1"
  :task_id "wave26-04-report-router-readiness-fields-v0"
  :status done
  :commit_hash "44ddf9dddd2edd5bee2404c68553a3f5d4d55906"
  :files_changed
    [".missiond/tasks/schema/report-contract-v1.lisp"
     "scripts/check-task-report.mjs"]
  :acceptance_results
    [(:command "node scripts/check-task-report.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "30/30 fixtures OK; 23 prior wave23-02 + wave25-02 + wave25-05 cases byte-identically green; +7 new wave26-04 cases (legacy-no-readiness pass; all-5-fields-populated pass with claudecode current-default seed shape; invalid status enum reject; eligible-as-string reject; runtime_allowed-as-string reject; absolute path reject; empty-string blocker reject)")
     (:command "node scripts/check-task-contract.mjs --all"
      :exit_code 0
      :ok true
      :notes "83 task contracts validated")
     (:command "git diff --check -- .missiond/tasks/schema/report-contract-v1.lisp scripts/check-task-report.mjs"
      :exit_code 0
      :ok true
      :notes "no whitespace or merge-conflict markers in the 2 staged files")]
  :scope_deviations []
  :notes "Strictly additive change. Schema gains 5 new optional flat top-level fields under (optional-report-fields ...) plus 5 new (field-contract ...) entries. Checker gains ALLOWED_ROUTER_READINESS Set + validateRouterLiteralBoolEither tight wrapper + validateRouterApplyBlockers helper, wired as 5 optional checks in validateReport. Reused wave25-02 helpers validateRouterEnumField + validateRouterRepoRelativePath for symmetry. node scripts/check-task-report.mjs --all (52 existing real reports) re-validates green — backward-compat verified end-to-end."
  :recommended_backend "claudecode"
  :router_confidence "high"
  :router_policy_path ".missiond/router/router-policy-v1.lisp"
  :router_dry_run_only true
  :router_applied false
  :router_reasons
    ["fresh-code-alignment dispatch strategy"
     "code-alignment kind matches r-claudecode-default rule"]
  :router_backend_readiness_status "current-default"
  :router_backend_runtime_allowed true
  :router_apply_eligible false
  :router_apply_blockers
    ["apply gate requires runtime-ready; current-default is NOT sufficient"
     "wave26 explicitly out-of-scope for runtime backend replacement"]
  :router_backend_registry_path ".missiond/router/router-backend-registry-v1.lisp"
  :time_sinks
    ["reading wave25-02 helpers to confirm validateRouterLiteralBool's single-atom signature before deciding wrapper-vs-refactor"
     (:label "appending shared-memory + session-trace ledger entries"
      :notes "tracked closing-paren accounting after appending the completion entry; one extra paren snuck in on first try and was corrected before the next acceptance step")]
  :major_decisions
    [(:decision "Add validateRouterLiteralBoolEither wrapper instead of widening validateRouterLiteralBool"
      :rationale "wave25-02's expectedAtom positional parameter is exercised by 2 hard-coded call sites that must stay byte-identical; widening would either force every caller to pass a Set or change semantics. A 7-line wrapper preserves wave25-02's stricter single-atom contract while adding the both-polarities case wave26-04 needs."
      :trace_ref "wave26-04-trace-commit-001")
     (:decision "Mirror validateRouterReasons pattern for validateRouterApplyBlockers (vector of non-empty strings, no property-list entries)"
      :rationale "wave26-01/02/03 all emit plain strings for blocker entries; allowing property-list entries would invite drift between report shape and registry shape.")]
  :unexpected_work
    [(:summary "Tightened the (checker-contract :rejects ...) list with 5 new rejection strings and updated the non-goal note to mention router-readiness fields are observational like router-recommendation"
      :trace_ref "wave26-04-trace-commit-001")]
  :blockers []
  :trace_refs
    ["wave26-04-trace-start-001"
     "wave26-04-trace-commit-001"
     "wave26-04-trace-complete-001"
     ".missiond/tasks/wave26/session-trace.lisp"]
  :router_trace_index_path ".missiond/v2/index/session-trace-index.json")
