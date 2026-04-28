;; Wave 27 task contract.

(task wave27-04-report-router-dispatch-descriptor-fields-v0
  :schema "missiond.task-contract.v1"
  :title "Report router dispatch descriptor fields v0"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave27-01-router-dispatch-descriptor-schema-v0"]
  :dispatch-strategy "fresh-code-alignment"
  :session-trace-writable true
  :router-policy-path ".missiond/router/router-policy-v1.lisp"
  :router-backend-registry-path ".missiond/router/router-backend-registry-v1.lisp"
  :goal "Extend report-contract v1 so workers can record router dispatch descriptor facts without claiming runtime backend execution."

  :write-scope
    [".missiond/tasks/schema/report-contract-v1.lisp"
     "scripts/check-task-report.mjs"]

  :must-not-touch
    ["crates/**"
     ".missiond/v2/**"
     ".missiond/router/**"
     ".missiond/tasks/wave27/wave27-*.lisp"
     ".missiond/claudecode/**"
     "scripts/check-router-dispatch-descriptor.mjs"
     "scripts/build-router-dispatch-descriptor.mjs"
     "scripts/recommend-task-backend.mjs"
     "scripts/evaluate-router-policy-corpus.mjs"
     "scripts/render-claudecode-task.mjs"]

  :requirements
    ["Add optional flat report fields for descriptor evidence: router_dispatch_descriptor_path, router_dispatch_descriptor_status, router_dispatch_backend, router_dispatch_eligible, router_dispatch_no_execution, router_dispatch_blockers."
     "Use strict literal atom booleans for router_dispatch_eligible and router_dispatch_no_execution; reject strings."
     "router_dispatch_descriptor_status must be a closed enum such as absent, built, invalid, registry_missing, blocked."
     "router_dispatch_backend must reuse the router backend enum."
     "router_dispatch_descriptor_path must be repo-relative when supplied."
     "router_dispatch_no_execution must be true whenever supplied; reject false."
     "Add fixtures without disturbing all existing wave19-wave26 report fixtures."]

  :acceptance
    ["node scripts/check-task-report.mjs --dry-fixture"
     "node scripts/check-task-report.mjs --all"
     "node scripts/check-task-contract.mjs --all"
     "git diff --check -- .missiond/tasks/schema/report-contract-v1.lisp scripts/check-task-report.mjs"]

  :commit
    (:required true
     :message "feat(tasks): record router dispatch descriptors in reports"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Added report fields and validation rules."
     "Fixture count."
     "Acceptance command results."])
