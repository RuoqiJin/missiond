;; Wave 25 coordination note.

(task wave25-07-parallel-dispatch-index
  :schema "missiond.task-contract.v1"
  :title "Wave 25 parallel dispatch index"
  :kind coordination
  :status ready
  :owner "codex-orchestrator"
  :depends-on []
  :dispatch-strategy "manual"
  :goal "Human/Codex dispatch index for Wave 25. This file is not meant for a ClaudeCode worker commit by itself."

  :write-scope
    [".missiond/tasks/wave25/wave25-*.lisp"
     ".missiond/claudecode/wave25-*.md"]

  :must-not-touch
    ["crates/**"
     "scripts/**"
     ".missiond/v2/**"
     ".missiond/tasks/wave24/**"]

  :requirements
    ["Group A: 00 archive must run first."
     "Group B after 00: 01 evaluator, 02 report fields, and 03 plan trace-index confidence can run in parallel (disjoint write scopes)."
     "Group C: 04 renderer depends on 01+02."
     "Group D: 05 smoke depends on 01+02+03+04."
     "Group E: 06 Lisp backfill is Codex-owned after all committed code tasks."
     "Wave25 remains dry-run/advisory only; no runtime backend replacement task is included."]

  :acceptance
    ["node scripts/check-task-contract.mjs --all"]

  :commit
    (:required false
     :message "docs(wave25): dispatch index"
     :scope-check not-required)

  :report
    ["No report required; this is a coordination index."])
