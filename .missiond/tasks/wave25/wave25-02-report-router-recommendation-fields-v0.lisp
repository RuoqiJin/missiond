;; Wave 25 task contract.

(task wave25-02-report-router-recommendation-fields-v0
  :schema "missiond.task-contract.v1"
  :title "Report-contract router recommendation fields v0"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave25-00-archive-wave24-artifacts"]
  :dispatch-strategy "fresh-code-alignment"
  :session-trace-writable true
  :router-policy-path ".missiond/router/router-policy-v1.lisp"
  :goal "Extend report-contract v1 so ClaudeCode completion reports can record the dry-run router recommendation they observed, without making the recommendation authoritative."

  :write-scope
    [".missiond/tasks/schema/report-contract-v1.lisp"
     "scripts/check-task-report.mjs"]

  :must-not-touch
    ["crates/**"
     "scripts/render-claudecode-task.mjs"
     "scripts/recommend-task-backend.mjs"
     "scripts/evaluate-router-policy-corpus.mjs"
     ".missiond/v2/**"
     ".missiond/tasks/wave24/**"
     ".missiond/tasks/wave25/wave25-*.lisp"
     ".missiond/claudecode/**"]

  :requirements
    ["Add optional report fields for router_recommendation or equivalent flat fields: recommended_backend, confidence, policy_path, dry_run_only, applied, reasons, and trace_index_path when known."
     "The checker must accept old reports with no router fields unchanged."
     "If router fields are present, enforce backend enum, confidence enum, dry_run_only=true, applied=false, and policy path repo-relative."
     "Reject any report that claims runtime replacement or applied=true."
     "Add dry fixtures for legacy report, valid router block, invalid backend, applied=true, dry_run_only=false, and absolute policy path."]

  :acceptance
    ["node scripts/check-task-report.mjs --dry-fixture"
     "node scripts/check-task-report.mjs .missiond/tasks/wave24/reports/wave24-04-plan-router-dry-run-surface-v0.report.lisp"
     "node scripts/check-task-contract.mjs --all"
     "git diff --check -- .missiond/tasks/schema/report-contract-v1.lisp scripts/check-task-report.mjs"]

  :commit
    (:required true
     :message "feat(tasks): record router recommendation in reports"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "New optional report fields and validation rules."
     "Backward-compat proof for old reports."
     "Acceptance command results."])

