;; Wave 26 task contract.

(task wave26-07-lisp-backfill-router-readiness-status
  :schema "missiond.task-contract.v1"
  :title "Lisp backfill router readiness status"
  :kind lisp-only
  :status ready
  :owner "codex"
  :depends-on ["wave26-06-router-readiness-smoke-v1"]
  :dispatch-strategy "resident-lisp"
  :router-policy-path ".missiond/router/router-policy-v1.lisp"
  :goal "Backfill v2 Lisp blueprints with Wave26 backend readiness registry and router apply-blocker facts after the code tasks are committed. This task is Codex-owned; do not send it to ClaudeCode unless explicitly redirected."

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
     ".missiond/claudecode/**"
     ".missiond/router/**"]

  :requirements
    ["Backfill only committed Wave26 facts; do not speculate beyond reports."
     "Mark backend registry/checker, recommendation readiness, mission_plan readiness, report fields, renderer context, and smoke according to actual reports."
     "Keep runtime router replacement pending unless a future task actually changes runtime dispatch."
     "Add source-index entries for each new router readiness artifact and status-upgrade entries for trace-derived-router-policy."]

  :acceptance
    ["node scripts/check-architecture-lisp.mjs --all-v2"
     "node scripts/check-task-contract.mjs --all"
     "git diff --check -- .missiond/v2/intent-machine-contract.lisp .missiond/v2/intent-workstation-policy.lisp .missiond/v2/intent-pillar-source-index.lisp .missiond/v2/intent-flow.lisp .missiond/v2/intent-intent-layer.lisp .missiond/v2/intent-tools.lisp .missiond/v2/intent.lisp"]

  :commit
    (:required true
     :message "docs(v2): backfill wave26 router readiness status"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Updated source-index anchors."
     "Remaining pending items."
     "Acceptance command results."])

