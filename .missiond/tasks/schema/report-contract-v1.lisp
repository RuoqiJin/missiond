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
  :status "code-aligned initial — checker implemented in scripts/check-task-report.mjs"
  :checker "scripts/check-task-report.mjs"

  (purpose
    "Make ClaudeCode task completion verifiable by structure, not narrative."
    "Allow MissionD plan/dispatch loops to reason about success/failure programmatically."
    "Pin the executed commit + the actual files changed so scope drift is detectable.")

  (required-report-fields
    [:schema :task_id :status :commit_hash :files_changed :acceptance_results])

  (optional-report-fields
    [:scope_deviations :notes])

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
    (:notes "free-form prose for humans; never load-bearing for verification."))

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
       "malformed acceptance entry"]
    :non-goal
      "checker does NOT execute the acceptance commands; it only validates structure."))
