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
  :status "code-aligned — checker scripts/check-task-report.mjs; full-run verifier scripts/verify-task-run.mjs; wave23-02 added optional worker-explanation fields (:time_sinks :major_decisions :unexpected_work :blockers :trace_refs) — prose only, structural validation, never SSOT for facts (facts live in session-trace.lisp)"
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
     :time_sinks :major_decisions :unexpected_work :blockers :trace_refs])

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
      "Used to link prose explanation back to factual telemetry; absolute paths are rejected."))

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
       "structured worker-explanation entries missing their declared key (:label / :decision / :summary)"]
    :non-goal
      "checker does NOT execute the acceptance commands; it only validates structure. Worker-explanation fields are prose-only — they are validated structurally but their content is never treated as ground truth."))
