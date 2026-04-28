;; Wave 27 task contract.

(task wave27-05-renderer-router-dispatch-descriptor-context-v0
  :schema "missiond.task-contract.v1"
  :title "Renderer router dispatch descriptor context v0"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave27-01-router-dispatch-descriptor-schema-v0"
               "wave27-02-router-dispatch-descriptor-cli-v0"
               "wave27-04-report-router-dispatch-descriptor-fields-v0"]
  :dispatch-strategy "fresh-code-alignment"
  :session-trace-writable true
  :router-policy-path ".missiond/router/router-policy-v1.lisp"
  :router-backend-registry-path ".missiond/router/router-backend-registry-v1.lisp"
  :goal "Teach the task renderer to surface router dispatch descriptor commands and report-field guidance while preserving the advisory/no-execution boundary."

  :write-scope
    ["scripts/render-claudecode-task.mjs"
     ".missiond/tasks/schema/task-contract-v1.lisp"]

  :must-not-touch
    ["crates/**"
     ".missiond/v2/**"
     ".missiond/router/**"
     ".missiond/tasks/wave27/wave27-*.lisp"
     ".missiond/claudecode/**"
     "scripts/check-router-dispatch-descriptor.mjs"
     "scripts/build-router-dispatch-descriptor.mjs"
     "scripts/check-task-report.mjs"
     "scripts/recommend-task-backend.mjs"
     "scripts/evaluate-router-policy-corpus.mjs"]

  :requirements
    ["Add optional task field :router-dispatch-descriptor-path / :router_dispatch_descriptor_path if useful; otherwise document why descriptor is generated on demand."
     "Render a read-only command using scripts/build-router-dispatch-descriptor.mjs and pipe/check instructions using scripts/check-router-dispatch-descriptor.mjs."
     "The rendered section must include literal phrases: advisory, dry-run only, no execution, and MUST NOT switch backend."
     "Report Contract section must mention the wave27-04 descriptor report fields with MAY language."
     "Renderer must not shell out and must preserve the wave26 Router Policy section ordering unless tests prove an intentional minimal insertion."
     "Add renderer dry-fixture coverage for descriptor literals and static audit for shell/child_process/git/network patterns."]

  :acceptance
    ["node scripts/render-claudecode-task.mjs --dry-fixture"
     "node scripts/render-claudecode-task.mjs --stdout .missiond/tasks/wave27/wave27-02-router-dispatch-descriptor-cli-v0.lisp > /tmp/wave27-router-dispatch-descriptor.md"
     "rg 'Router Policy|dispatch descriptor|no execution|MUST NOT switch backend|build-router-dispatch-descriptor' /tmp/wave27-router-dispatch-descriptor.md"
     "node scripts/check-task-contract.mjs --all"
     "git diff --check -- scripts/render-claudecode-task.mjs .missiond/tasks/schema/task-contract-v1.lisp"]

  :commit
    (:required true
     :message "feat(tasks): render router dispatch descriptor context"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Rendered command shape."
     "Renderer invariants."
     "Acceptance command results."])
