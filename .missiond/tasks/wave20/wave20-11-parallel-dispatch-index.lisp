;; Wave 20 task contract.

(task wave20-11-parallel-dispatch-index
  :schema "missiond.task-contract.v1"
  :title "Wave 20 parallel dispatch index"
  :kind docs
  :status ready
  :owner "codex-orchestrator"
  :depends-on []
  :dispatch-strategy "manual"
  :goal "Record Wave 20 dispatch groups and write-scope conflicts for operator-led parallel ClaudeCode sessions."

  :write-scope
    [".missiond/claudecode/wave20-11-parallel-dispatch-index.md"]

  :must-not-touch
    ["crates/**"
     "scripts/**"
     ".missiond/v2/*.lisp"
     ".missiond/tasks/wave20/*.lisp"]

  :requirements
    ["Create a concise dispatch index grouped by dependency and write-scope conflict."
     "Call out plan.rs, plan_dag.rs, agent_execution.rs, unified_entry.rs, and renderer/script conflicts."
     "Keep under 140 lines."]

  :acceptance
    ["git diff --check -- .missiond/claudecode/wave20-11-parallel-dispatch-index.md"
     "node scripts/check-task-contract.mjs --all"]

  :commit
    (:required false
     :scope-check not-required)

  :report
    ["Index path."
     "Recommended dispatch groups."
     "Acceptance command results."])
