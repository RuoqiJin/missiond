;; Wave 24 coordination note.

(task wave24-08-parallel-dispatch-index
  :schema "missiond.task-contract.v1"
  :title "Wave 24 parallel dispatch index"
  :kind coordination
  :status ready
  :owner "codex-orchestrator"
  :depends-on []
  :dispatch-strategy "manual"
  :goal "Human/Codex dispatch index for Wave 24. This file is not meant for a ClaudeCode worker commit by itself."

  :write-scope
    [".missiond/tasks/wave24/wave24-*.lisp"
     ".missiond/claudecode/wave24-*.md"]

  :must-not-touch
    ["crates/**"
     "scripts/**"
     ".missiond/v2/**"
     ".missiond/tasks/wave23/**"]

  :requirements
    ["Parallel group A: 00 archive can run alone first."
     "Parallel group B after 00: 01 router-policy schema/checker and 02 trace corpus index can run in parallel."
     "Sequential group C: 03 recommendation CLI depends on 01+02."
     "Parallel group D after 03: 04 plan dry-run surface and 05 renderer router context may run in parallel, but coordinate if both touch task-contract schema."
     "Group E: 06 smoke after 04+05."
     "Group F: 07 Lisp backfill is Codex-owned after all committed code tasks."]

  :acceptance
    ["node scripts/check-task-contract.mjs --all"]

  :commit
    (:required false
     :message "docs(wave24): dispatch index"
     :scope-check not-required)

  :report
    ["No report required; this is a coordination index."])
