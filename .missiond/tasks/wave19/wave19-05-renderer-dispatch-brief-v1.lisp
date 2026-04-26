;; Wave 19 task contract.

(task wave19-05-renderer-dispatch-brief-v1
  :schema "missiond.task-contract.v1"
  :title "Renderer dispatch brief v1"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave19-02-task-contract-verifier-v1" "wave19-03-report-contract-v1" "wave19-04-shared-memory-ledger-v0"]
  :dispatch-strategy "fresh-code-alignment"
  :goal "Upgrade the Lisp-to-Markdown renderer so ClaudeCode briefs carry enough machine-contract context for scoped commits and shared-memory handoff."

  :write-scope
    ["scripts/render-claudecode-task.mjs"
     ".missiond/tasks/schema/task-contract-v1.lisp"
     ".missiond/tasks/wave19/wave19-00-machine-contract-pilot.lisp"
     ".missiond/claudecode/wave19-00-machine-contract-pilot.md"]

  :must-not-touch
    ["crates/**"
     ".missiond/v2/*.lisp"
     "scripts/check-task-contract.mjs"
     "scripts/verify-task-contract.mjs"
     "scripts/check-task-report.mjs"
     "scripts/check-task-memory.mjs"]

  :requirements
    ["Render depends_on, dispatch_strategy, shared-memory path, report-contract expectation, and verify-task-contract command when available."
     "If :dispatch-strategy is agent-team, render the exact literal 使用 agent-team提高效率 once."
     "Keep existing output backward compatible for current fields."
     "Re-render wave19-00 pilot with --force as a golden example."]

  :acceptance
    ["node scripts/check-task-contract.mjs --all"
     "node scripts/render-claudecode-task.mjs --force .missiond/tasks/wave19/wave19-00-machine-contract-pilot.lisp"
     "git diff --check -- scripts/render-claudecode-task.mjs .missiond/tasks/schema/task-contract-v1.lisp .missiond/tasks/wave19/wave19-00-machine-contract-pilot.lisp .missiond/claudecode/wave19-00-machine-contract-pilot.md"]

  :commit
    (:required true
     :message "feat(tasks): enrich rendered dispatch briefs"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Rendered sections added."
     "Backward compatibility notes."
     "Acceptance command results."])
