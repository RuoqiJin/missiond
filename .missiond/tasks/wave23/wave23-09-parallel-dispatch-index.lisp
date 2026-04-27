;; Wave 23 task contract.

(task wave23-09-parallel-dispatch-index
  :schema "missiond.task-contract.v1"
  :title "Wave 23 parallel dispatch index"
  :kind docs
  :status ready
  :owner "codex-orchestrator"
  :depends-on []
  :dispatch-strategy "manual"
  :goal "Record Wave 23 dispatch groups and write-scope conflicts for operator-led parallel ClaudeCode sessions."

  :write-scope
    [".missiond/claudecode/wave23-09-parallel-dispatch-index.md"]

  :must-not-touch
    ["crates/**"
     "scripts/**"
     ".missiond/v2/*.lisp"
     ".missiond/tasks/wave23/*.lisp"]

  :requirements
    ["Create a concise dispatch index grouped by dependency and write-scope conflict."
     "Call out conflicts around session-trace scripts, agent_execution.rs, plan.rs, workstation_dispatch.rs, and v2 Lisp backfill."
     "Keep under 140 lines."]

  :acceptance
    ["git diff --check -- .missiond/claudecode/wave23-09-parallel-dispatch-index.md"
     "node scripts/check-task-contract.mjs --all"]

  :commit
    (:required false
     :scope-check not-required)

  :report
    ["Index path."
     "Recommended dispatch groups."
     "Acceptance command results."])
