;; Wave 23 task contract.

(task wave23-08-lisp-backfill-wave23-status
  :schema "missiond.task-contract.v1"
  :title "Lisp backfill Wave 23 status"
  :kind lisp-only
  :status ready
  :owner "codex-architect"
  :depends-on ["wave23-01-session-trace-schema-v0"
               "wave23-02-renderer-report-trace-fields-v1"
               "wave23-03-task-run-verifier-trace-v1"
               "wave23-04-execution-session-trace-integration-v0"
               "wave23-05-plan-workstation-session-trace-v0"
               "wave23-06-trace-summary-analyzer-v0"
               "wave23-07-router-policy-draft-from-trace-v0"]
  :dispatch-strategy "manual"
  :goal "Codex-owned architecture/status task: backfill MissionD v2 Lisp for Wave 23, marking session-trace collection and trace-derived router policy accurately while keeping router replacement pending."

  :write-scope
    [".missiond/v2/intent-machine-contract.lisp"
     ".missiond/v2/intent-pillar-source-index.lisp"
     ".missiond/v2/intent-flow.lisp"
     ".missiond/v2/intent-intent-layer.lisp"
     ".missiond/v2/intent-tools.lisp"
     ".missiond/v2/intent-workstation-policy.lisp"
     ".missiond/v2/intent-execution-governance.lisp"
     ".missiond/v2/intent.lisp"]

  :must-not-touch
    ["crates/**"
     "scripts/**"
     ".missiond/tasks/**"
     ".missiond/claudecode/**"]

  :requirements
    ["Do not delegate this architecture Lisp task to ClaudeCode; Codex owns the backfill."
     "Backfill only committed Wave23 facts."
     "Mark session-trace schema/checker/integration status separately from future trace analyzer/router replacement."
     "Keep frontend Lisp explicitly postponed."]

  :acceptance
    ["node scripts/check-architecture-lisp.mjs --all-v2"
     "node scripts/check-task-contract.mjs --all"
     "git diff --check -- .missiond/v2/intent-machine-contract.lisp .missiond/v2/intent-pillar-source-index.lisp .missiond/v2/intent-flow.lisp .missiond/v2/intent-intent-layer.lisp .missiond/v2/intent-tools.lisp .missiond/v2/intent-workstation-policy.lisp .missiond/v2/intent-execution-governance.lisp .missiond/v2/intent.lisp"]

  :commit
    (:required true
     :message "docs(v2): backfill wave23 session-trace status"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Anchors updated."
     "Trace collection vs router replacement distinction."
     "Remaining pending list."
     "Acceptance command results."])
