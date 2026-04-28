;; Wave 26 coordination note.

(task wave26-08-parallel-dispatch-index
  :schema "missiond.task-contract.v1"
  :title "Wave 26 parallel dispatch index"
  :kind coordination
  :status ready
  :owner "codex-orchestrator"
  :depends-on []
  :dispatch-strategy "manual"
  :goal "Human/Codex dispatch index for Wave 26. This file is not meant for a ClaudeCode worker commit by itself."

  :write-scope
    [".missiond/tasks/wave26/wave26-*.lisp"
     ".missiond/claudecode/wave26-*.md"]

  :must-not-touch
    ["crates/**"
     "scripts/**"
     ".missiond/v2/**"
     ".missiond/tasks/wave25/**"]

  :requirements
    ["Group A: 00 archive must run first."
     "Group B after 00: 01 backend registry is the foundation and must run before readiness consumers."
     "Group C after 01: 02 Node recommendation readiness and 03 Rust mission_plan readiness can run in parallel (disjoint write scopes)."
     "Group D after 02: 04 report readiness fields."
     "Group E after 01+02+04: 05 renderer readiness context."
     "Group F after 02+03+04+05: 06 smoke."
     "Group G: 07 Lisp backfill is Codex-owned after all committed code tasks."
     "Wave26 remains dry-run/advisory/readiness-only; no runtime backend replacement task is included."]

  :acceptance
    ["node scripts/check-task-contract.mjs --all"]

  :commit
    (:required false
     :message "docs(wave26): dispatch index"
     :scope-check not-required)

  :report
    ["No report required; this is a coordination index."])
