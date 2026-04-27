;; Wave 24 task contract.

(task wave24-05-renderer-router-context-v0
  :schema "missiond.task-contract.v1"
  :title "Renderer router context v0"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave24-01-router-policy-schema-v1"
               "wave24-03-router-recommendation-cli-v0"]
  :dispatch-strategy "fresh-code-alignment"
  :session-trace-writable true
  :goal "Surface router policy context in rendered ClaudeCode task briefs as advisory dry-run information, without making Markdown load-bearing or changing task-contract SSOT."

  :write-scope
    ["scripts/render-claudecode-task.mjs"
     ".missiond/tasks/schema/task-contract-v1.lisp"]

  :must-not-touch
    ["crates/**"
     ".missiond/v2/**"
     ".missiond/tasks/wave23/**"
     ".missiond/tasks/wave24/wave24-*.lisp"
     ".missiond/claudecode/wave23-*.md"]

  :requirements
    ["Add optional task-contract field for router_policy_path or router_context_path if needed; default absent."
     "Renderer may auto-detect .missiond/router/router-policy-v1.lisp and include a Router Policy section."
     "The rendered section must say advisory/dry-run only and must not instruct ClaudeCode to change backend."
     "Preserve existing Machine Contract, Session Trace, Report Contract, and Commit sections."
     "Do not run recommendation CLI from renderer unless the task contract explicitly provides a safe read-only input path; avoid hidden expensive work."]

  :acceptance
    ["node scripts/render-claudecode-task.mjs --stdout .missiond/tasks/wave24/wave24-01-router-policy-schema-v1.lisp >/tmp/wave24-router-render.md"
     "node scripts/check-task-contract.mjs --all"
     "git diff --check -- scripts/render-claudecode-task.mjs .missiond/tasks/schema/task-contract-v1.lisp"]

  :commit
    (:required true
     :message "feat(tasks): render router policy context"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Rendered section behavior."
     "Why Markdown remains non-load-bearing."
     "Acceptance command results."])
