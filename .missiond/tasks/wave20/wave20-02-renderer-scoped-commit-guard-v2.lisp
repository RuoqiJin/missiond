;; Wave 20 task contract.

(task wave20-02-renderer-scoped-commit-guard-v2
  :schema "missiond.task-contract.v1"
  :title "Renderer scoped commit guard v2"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave20-01-task-scope-index-guard-v1"]
  :dispatch-strategy "fresh-code-alignment"
  :goal "Make every rendered ClaudeCode brief show the scoped staging and pre-commit guard sequence so agents do not accidentally commit someone else's staged files."

  :write-scope
    ["scripts/render-claudecode-task.mjs"
     ".missiond/tasks/schema/task-contract-v1.lisp"
     ".missiond/tasks/wave20/wave20-00-archive-wave19-task-contracts.lisp"
     ".missiond/claudecode/wave20-00-archive-wave19-task-contracts.md"]

  :must-not-touch
    ["crates/**"
     ".missiond/v2/*.lisp"
     "scripts/task-scope-guard.mjs"
     ".githooks/pre-commit"]

  :requirements
    ["Render a pre-commit scoped-index check before git commit whenever :commit :required true."
     "Render MISSIOND_TASK_CONTRACT=<task.lisp> as the hook activation environment."
     "Keep the existing verify-task-contract post-commit command."
     "Re-render wave20-00 as the golden example."]

  :acceptance
    ["node scripts/check-task-contract.mjs --all"
     "node scripts/render-claudecode-task.mjs --force .missiond/tasks/wave20/wave20-00-archive-wave19-task-contracts.lisp"
     "git diff --check -- scripts/render-claudecode-task.mjs .missiond/tasks/schema/task-contract-v1.lisp .missiond/tasks/wave20/wave20-00-archive-wave19-task-contracts.lisp .missiond/claudecode/wave20-00-archive-wave19-task-contracts.md"]

  :commit
    (:required true
     :message "feat(tasks): render scoped commit guard steps"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Rendered commit-section changes."
     "Golden brief path."
     "Acceptance command results."])
