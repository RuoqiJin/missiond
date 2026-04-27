;; Wave 21 task contract.

(task wave21-10-parallel-dispatch-index
  :schema "missiond.task-contract.v1"
  :title "Wave 21 parallel dispatch index"
  :kind docs
  :status ready
  :owner "codex-orchestrator"
  :depends-on []
  :dispatch-strategy "manual"
  :goal "Record Wave 21 dispatch groups and write-scope conflicts for operator-led parallel ClaudeCode sessions."

  :write-scope
    [".missiond/claudecode/wave21-10-parallel-dispatch-index.md"]

  :must-not-touch
    ["crates/**"
     "scripts/**"
     ".missiond/v2/*.lisp"
     ".missiond/tasks/wave21/*.lisp"]

  :requirements
    ["Create a concise dispatch index grouped by dependency and write-scope conflict."
     "Call out conflicts around plan.rs, agent_execution.rs, workflow.rs, review_gate.rs, and shared scripts."
     "Keep under 140 lines."]

  :acceptance
    ["git diff --check -- .missiond/claudecode/wave21-10-parallel-dispatch-index.md"
     "node scripts/check-task-contract.mjs --all"]

  :commit
    (:required false
     :scope-check not-required)

  :report
    ["Index path."
     "Recommended dispatch groups."
     "Acceptance command results."])
