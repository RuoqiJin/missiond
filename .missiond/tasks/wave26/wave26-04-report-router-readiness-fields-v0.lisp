;; Wave 26 task contract.

(task wave26-04-report-router-readiness-fields-v0
  :schema "missiond.task-contract.v1"
  :title "Report router readiness fields v0"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave26-02-router-recommendation-readiness-v1"]
  :dispatch-strategy "fresh-code-alignment"
  :session-trace-writable true
  :router-policy-path ".missiond/router/router-policy-v1.lisp"
  :goal "Extend report-contract v1 so workers can record backend readiness and apply blockers from the Wave26 router readiness loop."

  :write-scope
    [".missiond/tasks/schema/report-contract-v1.lisp"
     "scripts/check-task-report.mjs"]

  :must-not-touch
    ["crates/**"
     "scripts/render-claudecode-task.mjs"
     "scripts/recommend-task-backend.mjs"
     "scripts/evaluate-router-policy-corpus.mjs"
     ".missiond/router/**"
     ".missiond/v2/**"
     ".missiond/tasks/wave25/**"
     ".missiond/tasks/wave26/wave26-*.lisp"
     ".missiond/claudecode/**"]

  :requirements
    ["Add optional flat fields: :router_backend_readiness_status, :router_backend_runtime_allowed, :router_apply_eligible, :router_apply_blockers, :router_backend_registry_path."
     "Validate readiness_status enum: current-default, advisory-only, runtime-ready, unavailable, unknown."
     "router_backend_runtime_allowed and router_apply_eligible must be literal atom booleans, not strings."
     "router_apply_blockers must be a vector of non-empty strings."
     "router_backend_registry_path must be repo-relative if supplied."
     "Legacy reports without these fields must remain valid."]

  :acceptance
    ["node scripts/check-task-report.mjs --dry-fixture"
     "node scripts/check-task-contract.mjs --all"
     "git diff --check -- .missiond/tasks/schema/report-contract-v1.lisp scripts/check-task-report.mjs"]

  :commit
    (:required true
     :message "feat(tasks): record router readiness in reports"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "New optional report fields."
     "Fixture count and validation coverage."
     "Acceptance command results."])

