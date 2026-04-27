;; Wave 24 task contract.

(task wave24-07-lisp-backfill-router-dry-run-status
  :schema "missiond.task-contract.v1"
  :title "Lisp backfill router dry-run status"
  :kind lisp-only
  :status ready
  :owner "codex-architect"
  :depends-on ["wave24-01-router-policy-schema-v1"
               "wave24-02-trace-corpus-index-v0"
               "wave24-03-router-recommendation-cli-v0"
               "wave24-04-plan-router-dry-run-surface-v0"
               "wave24-05-renderer-router-context-v0"
               "wave24-06-router-dry-run-smoke-v0"]
  :dispatch-strategy "manual"
  :goal "Codex-owned architecture/status backfill: record Wave 24 router dry-run artifacts in v2 Lisp without claiming runtime backend replacement."

  :write-scope
    [".missiond/v2/intent-machine-contract.lisp"
     ".missiond/v2/intent-workstation-policy.lisp"
     ".missiond/v2/intent-pillar-source-index.lisp"
     ".missiond/v2/intent-flow.lisp"
     ".missiond/v2/intent-intent-layer.lisp"
     ".missiond/v2/intent-tools.lisp"
     ".missiond/v2/intent.lisp"]

  :must-not-touch
    ["crates/**"
     "scripts/**"
     ".missiond/tasks/**"
     ".missiond/claudecode/**"]

  :requirements
    ["Do not delegate this blueprint/status task to ClaudeCode; Codex owns it."
     "Backfill only committed Wave24 facts."
     "Mark router recommendation CLI and mission_plan dry-run surface separately from future runtime router apply."
     "Keep frontend Lisp explicitly postponed unless a future wave starts it."]

  :acceptance
    ["node scripts/check-architecture-lisp.mjs --all-v2"
     "node scripts/check-task-contract.mjs --all"
     "git diff --check -- .missiond/v2/intent-machine-contract.lisp .missiond/v2/intent-workstation-policy.lisp .missiond/v2/intent-pillar-source-index.lisp .missiond/v2/intent-flow.lisp .missiond/v2/intent-intent-layer.lisp .missiond/v2/intent-tools.lisp .missiond/v2/intent.lisp"]

  :commit
    (:required true
     :message "docs(v2): backfill wave24 router dry-run status"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Anchors updated."
     "Router dry-run vs runtime replacement distinction."
     "Acceptance command results."])
