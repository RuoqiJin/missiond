;; Wave 19 task contract.

(task wave19-13-parallel-dispatch-index
  :schema "missiond.task-contract.v1"
  :title "Wave 19 parallel dispatch index"
  :kind docs
  :status ready
  :owner "codex-orchestrator"
  :depends-on []
  :dispatch-strategy "manual"
  :goal "Record the recommended parallel dispatch order for Wave 19 Lisp task contracts."

  :write-scope
    [".missiond/claudecode/wave19-13-parallel-dispatch-index.md"]

  :must-not-touch
    ["crates/**"
     ".missiond/v2/*.lisp"
     ".missiond/tasks/wave19/*.lisp"
     "scripts/**"]

  :requirements
    ["Create or refresh a short Markdown index grouping tasks by dependency and write-scope conflict."
     "List tasks that must not run together because they touch agent_execution.rs or plan.rs."
     "Keep it under 120 lines."]

  :acceptance
    ["git diff --check -- .missiond/claudecode/wave19-13-parallel-dispatch-index.md"
     "node scripts/check-task-contract.mjs --all"]

  :commit
    (:required false
     :scope-check not-required)

  :report
    ["Index path."
     "Recommended dispatch groups."
     "Acceptance command results."])
