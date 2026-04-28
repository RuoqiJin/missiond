;; Wave 25 task contract.

(task wave25-04-renderer-router-recommendation-command-v1
  :schema "missiond.task-contract.v1"
  :title "Renderer router recommendation command v1"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave25-01-router-policy-corpus-evaluator-v0"
               "wave25-02-report-router-recommendation-fields-v0"]
  :dispatch-strategy "fresh-code-alignment"
  :session-trace-writable true
  :router-policy-path ".missiond/router/router-policy-v1.lisp"
  :goal "Make rendered task briefs show exact read-only router recommendation commands and report-field expectations, while keeping the recommendation advisory and non-load-bearing."

  :write-scope
    ["scripts/render-claudecode-task.mjs"
     ".missiond/tasks/schema/task-contract-v1.lisp"]

  :must-not-touch
    ["crates/**"
     "scripts/check-task-report.mjs"
     "scripts/recommend-task-backend.mjs"
     "scripts/evaluate-router-policy-corpus.mjs"
     ".missiond/tasks/schema/report-contract-v1.lisp"
     ".missiond/v2/**"
     ".missiond/tasks/wave24/**"
     ".missiond/tasks/wave25/wave25-*.lisp"
     ".missiond/claudecode/**"]

  :requirements
    ["Extend the existing Router Policy (advisory) section with exact read-only commands: check-router-policy and recommend-task-backend for the current task."
     "If a report_contract path is rendered, mention that reports may include the optional router recommendation fields from wave25-02."
     "Do not make the recommendation a dispatch instruction; keep literal advisory and dry-run only text."
     "Do not shell out from the renderer; commands are text only."
     "Preserve Wave23 Session Trace section and Wave24 Router Policy section ordering and backward compatibility."]

  :acceptance
    ["node scripts/render-claudecode-task.mjs --stdout .missiond/tasks/wave25/wave25-01-router-policy-corpus-evaluator-v0.lisp > /tmp/wave25-render-check.md"
     "rg -n \"Router Policy|advisory|dry-run only|recommend-task-backend|check-router-policy\" /tmp/wave25-render-check.md"
     "node scripts/check-task-contract.mjs --all"
     "node scripts/check-task-report.mjs --dry-fixture"
     "git diff --check -- scripts/render-claudecode-task.mjs .missiond/tasks/schema/task-contract-v1.lisp"]

  :commit
    (:required true
     :message "feat(tasks): render router recommendation commands"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Rendered section wording and command examples."
     "Backward-compat proof."
     "Acceptance command results."])

