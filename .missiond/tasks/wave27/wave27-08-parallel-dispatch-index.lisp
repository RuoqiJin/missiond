;; Wave 27 coordination note.

(task wave27-08-parallel-dispatch-index
  :schema "missiond.task-contract.v1"
  :title "Wave 27 parallel dispatch index"
  :kind coordination
  :status ready
  :owner "codex-orchestrator"
  :depends-on []
  :dispatch-strategy "manual"
  :goal "Human/Codex dispatch index for Wave 27. This file is not meant for a ClaudeCode worker commit by itself."

  :write-scope
    [".missiond/tasks/wave27/wave27-*.lisp"
     ".missiond/claudecode/wave27-*.md"]

  :must-not-touch
    ["crates/**"
     "scripts/**"
     ".missiond/v2/**"
     ".missiond/tasks/wave26/**"]

  :requirements
    ["Group A: 00 archive must run first."
     "Group B after 00: 01 descriptor schema/checker is the foundation."
     "Group C after 01: 02 descriptor CLI, 03 mission_plan descriptor surface, and 04 report descriptor fields can run in parallel (disjoint primary write scopes)."
     "Group D after 01+02+04: 05 renderer descriptor context."
     "Group E after 02+03+04+05: 06 smoke."
     "Group F: 07 Lisp backfill is Codex-owned after all committed code tasks."
     "Wave27 remains descriptor/no-execution only; no runtime backend replacement task is included."]

  :acceptance
    ["node scripts/check-task-contract.mjs --all"]

  :commit
    (:required false
     :message "docs(wave27): dispatch index"
     :scope-check not-required)

  :report
    ["No report required; this is a coordination index."])
