;; MissionD report-contract v1
;; Purpose: machine-readable completion reports so that ClaudeCode results
;; can be verified mechanically without parsing free-form prose.
;;
;; A report is the SSOT counterpart of a task-contract dispatch:
;;   .missiond/tasks/<wave>/<task>.lisp           — task contract (input)
;;   .missiond/tasks/<wave>/reports/<task>.report.lisp — report contract (output)
;;
;; The renderer/orchestrator may project this Lisp into Markdown for humans,
;; but planners, distillers, and audits must consume the Lisp directly.

(report-contract-schema missiond.report-contract.v1
  :version "v1"
  :status "code-aligned — checker scripts/check-task-report.mjs; full-run verifier scripts/verify-task-run.mjs; wave23-02 added optional worker-explanation fields (:time_sinks :major_decisions :unexpected_work :blockers :trace_refs) — prose only, structural validation, never SSOT for facts (facts live in session-trace.lisp); wave25-02 added optional router-recommendation fields (:recommended_backend :router_confidence :router_policy_path :router_dry_run_only :router_applied :router_reasons :router_trace_index_path) — flat surface, mirrors the wave24-04 daemon dry-run block, additive-only, recommendation is NEVER authoritative; wave26-04 added optional router-readiness fields (:router_backend_readiness_status :router_backend_runtime_allowed :router_apply_eligible :router_apply_blockers :router_backend_registry_path) — flat surface, mirrors the wave26-02/03 backend readiness annotations, additive-only, runtime_allowed/apply_eligible MUST be literal atom booleans; wave27-04 added optional router-dispatch-descriptor fields (:router_dispatch_descriptor_path :router_dispatch_descriptor_status :router_dispatch_backend :router_dispatch_eligible :router_dispatch_no_execution :router_dispatch_blockers) — flat surface, mirrors the wave27-01 missiond.router-dispatch-descriptor.v1 head plus the wave27-02 builder output, additive-only, eligible MUST be literal atom true|false, no_execution MUST be literal atom true (false AND strings rejected — cross-wave invariant); wave29 prep adds optional commit-lineage fields (:agent_commit_hash :final_commit_hash :verified_commit_hash :parent_patches) so parent hotfixes can be recorded without amending worker commits; wave30-01 pins orchestrator-owned parent hotfix finalization through scripts/task-runner-finalize-report.mjs + scripts/task-runner-parent-hotfix.mjs; final/verified hashes must hex-prefix-agree with :commit_hash when present; wave40-01 promotes parent-hotfix finalization to a sparse Lisp projection that preserves the worker report's existing non-lineage keyword fields (notes / acceptance entries / worker-explanation fields / router-recommendation / readiness / dispatch-descriptor / verification-receipts and any future additive optional fields) and only patches the lineage fields :status :commit_hash :agent_commit_hash :final_commit_hash :verified_commit_hash :parent_patches and the unioned :files_changed; --acceptance-command appends new acceptance entries by default and only replaces the worker block when an explicit replacement opt is supplied"
  :checker "scripts/check-task-report.mjs"
  :run-verifier "scripts/verify-task-run.mjs"

  (purpose
    "Make ClaudeCode task completion verifiable by structure, not narrative."
    "Allow MissionD plan/dispatch loops to reason about success/failure programmatically."
    "Pin the executed commit + the actual files changed so scope drift is detectable.")

  (required-report-fields
    [:schema :task_id :status :commit_hash :files_changed :acceptance_results])

  (optional-report-fields
    [:scope_deviations :notes
     ;; wave23-02: worker explanation fields. Prose-only — never the SSOT
     ;; for facts. Facts live in .missiond/tasks/<wave>/session-trace.lisp.
     :time_sinks :major_decisions :unexpected_work :blockers :trace_refs
     ;; wave25-02: router-recommendation fields. Mirror the wave24-04 daemon
     ;; dry-run block shape on the report side. Strictly observational —
     ;; recording what the dry-run recommended, never overriding the actual
     ;; backend that ran. Cross-wave invariants enforced structurally:
     ;; applied MUST be literal false, dry_run_only MUST be literal true.
     :recommended_backend :router_confidence :router_policy_path
     :router_dry_run_only :router_applied :router_reasons
     :router_trace_index_path
     ;; wave26-04: router-readiness fields. Mirror the wave26-02 / wave26-03
     ;; backend readiness annotations on the report side so completion reports
     ;; can RECORD the observed backend readiness + apply-blockers without the
     ;; recording ever being treated as runtime authoritative. All five fields
     ;; are optional and additive; legacy reports without them remain valid.
     ;; Cross-wave invariants enforced structurally:
     ;;   runtime_allowed / apply_eligible MUST be literal atom booleans.
     :router_backend_readiness_status :router_backend_runtime_allowed
     :router_apply_eligible :router_apply_blockers
     :router_backend_registry_path
     ;; wave27-04: router-dispatch-descriptor fields. Mirror the wave27-01
     ;; missiond.router-dispatch-descriptor.v1 schema head plus the wave27-02
     ;; build-router-dispatch-descriptor.mjs output on the report side so
     ;; completion reports can RECORD the descriptor a task was associated
     ;; with WITHOUT claiming runtime backend execution happened. All six
     ;; fields are optional and additive; legacy reports without them remain
     ;; valid. Cross-wave invariants enforced structurally:
     ;;   :router_dispatch_eligible      — literal atom true|false (strings rejected)
     ;;   :router_dispatch_no_execution  — literal atom true ONLY (false AND
     ;;                                    strings rejected — the descriptor
     ;;                                    layer is locked no-execution).
    :router_dispatch_descriptor_path :router_dispatch_descriptor_status
    :router_dispatch_backend :router_dispatch_eligible
    :router_dispatch_no_execution :router_dispatch_blockers
    ;; wave29 prep: parent-hotfix / commit-lineage fields. :commit_hash stays
    ;; the final effective commit so legacy readers still verify the final
    ;; tree; these optional fields preserve the worker commit and hotfix trace.
    :agent_commit_hash :final_commit_hash :verified_commit_hash
    :parent_patches])

  (field-contract
    (:schema "must equal missiond.report-contract.v1")
    (:id "second form of (report <id> ...); equals task_id by convention")
    (:task_id "string; matches the originating (task <id> ...) form id")
    (:status "draft | in-progress | done | blocked | rejected")
    (:commit_hash "string; full or short git SHA. Empty allowed only when status != done")
    (:files_changed "vector of repo-relative paths; absolute paths are rejected")
    (:acceptance_results
      "vector of property lists, each (:command <string> :exit_code <int> :ok <bool> [:notes <string>])."
      "Must be non-empty when :status = done.")
    (:scope_deviations
      "vector of property lists describing files written outside :write-scope."
      "Each entry: (:path <string> :reason <string> [:approved_by <string>])."
      "Empty/omitted means no deviation.")
    (:notes "free-form prose for humans; never load-bearing for verification.")
    (:time_sinks
      "Optional. Vector describing where worker time went."
      "Each entry is a string OR a property list (:label <string> [:duration_ms <int>] [:notes <string>])."
      "Prose explanation; the canonical timing facts live in session-trace.lisp.")
    (:major_decisions
      "Optional. Vector describing material decisions made during the task."
      "Each entry is a string OR a property list (:decision <string> [:rationale <string>] [:trace_ref <string>])."
      "Prose explanation; decisions are not auto-applied by any planner.")
    (:unexpected_work
      "Optional. Vector describing scope surprises / extra work taken on."
      "Each entry is a string OR a property list (:summary <string> [:trace_ref <string>]).")
    (:blockers
      "Optional. Vector describing blockers encountered (resolved or open)."
      "Each entry is a string OR a property list (:summary <string> [:resolved <bool>] [:trace_ref <string>]).")
    (:trace_refs
      "Optional. Vector of session-trace event ids OR repo-relative paths to trace files."
      "Used to link prose explanation back to factual telemetry; absolute paths are rejected.")
    (:recommended_backend
      "Optional. wave25-02 router-recommendation surface."
      "String enum — one of: claudecode | missiond-llm-router | deterministic-checker | patch-worker | verifier-worker."
      "Records the backend the dry-run router recommended; NEVER authoritative.")
    (:router_confidence
      "Optional. wave25-02 router-recommendation surface."
      "String enum — one of: high | medium | low.")
    (:router_policy_path
      "Optional. wave25-02 router-recommendation surface."
      "String; repo-relative path to the router-policy file consulted (no leading '/' or '~', no '..' traversal).")
    (:router_dry_run_only
      "Optional. wave25-02 router-recommendation surface."
      "Cross-wave invariant: MUST be the literal atom true when present (string forms or any other value rejected).")
    (:router_applied
      "Optional. wave25-02 router-recommendation surface."
      "Cross-wave invariant: MUST be the literal atom false when present. Reports declaring true are rejected — runtime replacement is out of scope for wave 25.")
    (:router_reasons
      "Optional. wave25-02 router-recommendation surface."
      "Vector of non-empty strings; mirrors the daemon block's reasons array (matched rule ids / fallback notes / rejection messages).")
    (:router_trace_index_path
      "Optional. wave25-02 router-recommendation surface."
      "String; repo-relative path to the trace-index file used to compute confidence (no leading '/' or '~', no '..' traversal).")
    (:router_backend_readiness_status
      "Optional. wave26-04 router-readiness surface."
      "String enum — one of: current-default | advisory-only | runtime-ready | unavailable | unknown."
      "Mirrors the wave26-01 backend-registry :readiness_status atom plus the wave26-02/03 'unknown' fallback used when a backend lookup fails. Records what the registry observed; never authoritative for dispatch.")
    (:router_backend_runtime_allowed
      "Optional. wave26-04 router-readiness surface."
      "Cross-wave invariant: MUST be the literal atom true or false when present (string forms or any other value rejected)."
      "Mirrors the wave26-01 :runtime_allowed atom; observational, never authoritative.")
    (:router_apply_eligible
      "Optional. wave26-04 router-readiness surface."
      "Cross-wave invariant: MUST be the literal atom true or false when present (string forms or any other value rejected)."
      "Mirrors the wave26-02/03 strict apply-eligible gate output; observational only — current-default alone is NEVER sufficient (explicit runtime-ready opt-in required upstream).")
    (:router_apply_blockers
      "Optional. wave26-04 router-readiness surface."
      "Vector of non-empty strings; each entry names a concrete reason promotion to live dispatch is rejected. Mirrors the wave26-01 backend-registry :apply_blockers vector and the synthetic blockers wave26-02/03 splice in for unknown_backend / runtime_allowed=false / readiness_status!=runtime-ready outcomes.")
    (:router_backend_registry_path
      "Optional. wave26-04 router-readiness surface."
      "String; repo-relative path to the backend-registry file consulted (no leading '/' or '~', no '..' traversal). Mirrors the wave26-02 --backend-registry flag echo and the wave26-03 router_backend_registry_path MCP arg echo.")
    (:router_dispatch_descriptor_path
      "Optional. wave27-04 router-dispatch-descriptor surface."
      "String; repo-relative path to the descriptor artifact recorded for this task (no leading '/' or '~', no '..' traversal). Mirrors the wave27-02 builder output location.")
    (:router_dispatch_descriptor_status
      "Optional. wave27-04 router-dispatch-descriptor surface."
      "String enum — one of: absent | built | invalid | registry_missing | blocked. Records the descriptor lifecycle state observed when the report was assembled; observational only — never authoritative for dispatch.")
    (:router_dispatch_backend
      "Optional. wave27-04 router-dispatch-descriptor surface."
      "String enum — one of: claudecode | missiond-llm-router | deterministic-checker | patch-worker | verifier-worker. Reuses the wave25-02 router backend enum. Records which backend the descriptor declared; recording NEVER promotes the backend to authoritative dispatch.")
    (:router_dispatch_eligible
      "Optional. wave27-04 router-dispatch-descriptor surface."
      "Cross-wave invariant: MUST be the literal atom true or false when present (string forms or any other value rejected). Mirrors the wave27-01 :apply_eligible eligibility-gate output (runtime-ready + runtime_allowed=true + confidence=high + zero blockers); current-default alone is NEVER sufficient — observational only.")
    (:router_dispatch_no_execution
      "Optional. wave27-04 router-dispatch-descriptor surface."
      "Cross-wave invariant: MUST be the literal atom true when present (the literal atom false AND any string form are rejected). Mirrors the wave27-01 :no_execution invariant — recording a descriptor never asserts a runtime backend swap happened, only that the handoff fact was captured.")
    (:router_dispatch_blockers
      "Optional. wave27-04 router-dispatch-descriptor surface."
      "Vector of non-empty strings; each entry names a concrete reason the descriptor's dispatch handoff is blocked / advisory only. Mirrors the wave27-01 :blockers vector and the wave27-02 builder splice for absent / invalid / registry_missing / non-eligible outcomes.")
    (:agent_commit_hash
      "Optional. Git SHA string for the worker's original commit when a parent/orchestrator hotfix creates a later final commit. Hex string >= 7 and <= 64 chars; observational, never authoritative for dispatch.")
    (:final_commit_hash
      "Optional. Git SHA string for the final effective commit. When present it MUST hex-prefix-agree with :commit_hash so legacy readers still see the final commit.")
    (:verified_commit_hash
      "Optional. Git SHA string for the commit whose acceptance/scope verification was rerun. When present it MUST hex-prefix-agree with :commit_hash.")
    (:parent_patches
      "Optional. Vector of property lists, each (parent-patch :commit <sha> :kind <atom> :reason <string> :files [repo-relative paths...])."
      "Cross-wave invariants enforced structurally:"
      "  :commit          — hex string >= 7 and <= 64 chars (mirrors :agent_commit_hash)."
      "  :kind            — atom enum, one of: lint-cleanup | doc-fix | test-fix | scope-trim | hotfix-other."
      "  :reason          — non-empty string (human-readable why)."
      "  :files           — non-empty vector of repo-relative paths (no leading '/' or '~', no '..' traversal)."
      "  Final-hash drift — when :commit_hash and the LAST :parent_patches entry's :commit are both present, they MUST agree (hex prefix-match >= 7 chars). Worker commit before any patches is recorded in :agent_commit_hash; the final :commit_hash should equal the most recent parent patch commit."
      "  Wave30-01 actor boundary — when the parent/orchestrator applies a post-worker hotfix after the worker session exits, the orchestrator owns rewriting the report through task-runner-finalize-report.mjs / task-runner-parent-hotfix.mjs. The worker commit is preserved in :agent_commit_hash; :commit_hash, :final_commit_hash, and :verified_commit_hash point at the finalized commit."
      "  Wave40-01 sparse-projection invariant — task-runner-finalize-report.mjs MUST treat finalization as a sparse Lisp projection over the worker report. The finalizer parses the worker report's keyword/value pairs and re-emits them as-is, patching only the lineage fields (:status :commit_hash :agent_commit_hash :final_commit_hash :verified_commit_hash :parent_patches plus the unioned :files_changed). Optional fields already present in the worker report — including :notes :verification_tier :time_sinks :major_decisions :unexpected_work :blockers :trace_refs and the router-recommendation / router-readiness / router-dispatch-descriptor / verification-receipt blocks AND any additive optional fields a future wave introduces — MUST survive finalization unchanged. :acceptance_results is preserved by default; --acceptance-command (or appended programmatic entries) MUST append rather than replace unless an explicit replacement opt is supplied."))

  (status-contract
    :allowed [draft in-progress done blocked rejected]
    :done-requires
      [:commit_hash :files_changed :acceptance_results]
    :done-rules
      ["acceptance_results must be non-empty"
       "every acceptance entry must declare :ok and :exit_code"
       "files_changed paths must be repo-relative (no leading '/' or '~')"
       "scope_deviations entries each require :path and :reason"])

  (checker-contract
    :input ".missiond/tasks/**/reports/*.report.lisp"
    :modes [single-file dry-fixture all]
    :rejects
      ["missing :task_id"
       "invalid :status"
       "empty :acceptance_results when :status = done"
       "absolute paths in :files_changed or :scope_deviations"
       "schema mismatch"
       "malformed acceptance entry"
       ":time_sinks / :major_decisions / :unexpected_work / :blockers / :trace_refs not a vector when present"
       "absolute paths inside :trace_refs"
       "structured worker-explanation entries missing their declared key (:label / :decision / :summary)"
       ":recommended_backend not in {claudecode, missiond-llm-router, deterministic-checker, patch-worker, verifier-worker}"
       ":router_confidence not in {high, medium, low}"
       ":router_dry_run_only not the literal atom true"
       ":router_applied not the literal atom false (cross-wave invariant — runtime replacement is rejected)"
       "absolute or ~/.. paths inside :router_policy_path or :router_trace_index_path"
       ":router_reasons not a vector of non-empty strings"
       ":router_backend_readiness_status not in {current-default, advisory-only, runtime-ready, unavailable, unknown}"
       ":router_backend_runtime_allowed not the literal atom true or false (cross-wave invariant — strings are rejected)"
       ":router_apply_eligible not the literal atom true or false (cross-wave invariant — strings are rejected)"
       ":router_apply_blockers not a vector of non-empty strings"
       "absolute or ~/.. paths inside :router_backend_registry_path"
       ":router_dispatch_descriptor_status not in {absent, built, invalid, registry_missing, blocked}"
       ":router_dispatch_backend not in {claudecode, missiond-llm-router, deterministic-checker, patch-worker, verifier-worker}"
       ":router_dispatch_eligible not the literal atom true or false (cross-wave invariant — strings are rejected)"
       ":router_dispatch_no_execution not the literal atom true (cross-wave invariant — false AND strings are rejected)"
       ":router_dispatch_blockers not a vector of non-empty strings"
       "absolute or ~/.. paths inside :router_dispatch_descriptor_path"
       ":agent_commit_hash / :final_commit_hash / :verified_commit_hash malformed (hex string >= 7 and <= 64 chars)"
       ":final_commit_hash or :verified_commit_hash present but not hex-prefix-agreeing with :commit_hash"
       ":parent_patches entry missing :commit / :kind / :reason / non-empty :files"
       ":parent_patches :kind not in {lint-cleanup, doc-fix, test-fix, scope-trim, hotfix-other}"
       ":parent_patches entry :files contains absolute / ~ / .. path"
       ":parent_patches last entry :commit and report :commit_hash present but disagree (final-hash drift)"]
    :non-goal
      "checker does NOT execute the acceptance commands; it only validates structure. Worker-explanation fields are prose-only — they are validated structurally but their content is never treated as ground truth. Router-recommendation, router-readiness, and router-dispatch-descriptor fields are observational — the recorded recommendation / readiness / descriptor signals are NEVER promoted to authoritative dispatch by the checker."))
