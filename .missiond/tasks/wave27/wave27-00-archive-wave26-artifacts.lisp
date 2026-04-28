;; Wave 27 task contract.

(task wave27-00-archive-wave26-artifacts
  :schema "missiond.task-contract.v1"
  :title "Archive Wave 26 artifacts"
  :kind archive
  :status ready
  :owner "claudecode"
  :depends-on ["wave26-07-lisp-backfill-router-readiness-status"]
  :dispatch-strategy "fresh-code-alignment"
  :session-trace-writable true
  :router-policy-path ".missiond/router/router-policy-v1.lisp"
  :router-backend-registry-path ".missiond/router/router-backend-registry-v1.lisp"
  :goal "Archive all untracked Wave 26 task contracts, rendered ClaudeCode briefs, reports, shared-memory ledger, and session-trace ledger after Wave 26 code and Lisp commits are complete."

  :write-scope
    [".missiond/tasks/wave26/**"
     ".missiond/claudecode/wave26-*.md"]

  :must-not-touch
    ["crates/**"
     "scripts/**"
     ".missiond/v2/**"
     ".missiond/router/**"
     ".missiond/tasks/wave27/wave27-*.lisp"
     ".missiond/claudecode/wave27-*.md"]

  :requirements
    ["Stage only Wave 26 artifacts currently untracked."
     "Do not modify artifact contents while archiving; commit verbatim."
     "Include task contracts, rendered briefs, reports, shared-memory.lisp, and session-trace.lisp."
     "It is acceptable that Wave26 Codex-owned Lisp backfill and coordination-index tasks have no report; call that out explicitly if true."
     "Do not stage Wave 27 task files or briefs."]

  :acceptance
    ["node scripts/check-task-contract.mjs --all"
     "node scripts/check-task-memory.mjs .missiond/tasks/wave26/shared-memory.lisp"
     "node scripts/check-session-trace.mjs .missiond/tasks/wave26/session-trace.lisp"
     "git diff --cached --name-only"
     "git diff --check -- .missiond/tasks/wave26 .missiond/claudecode"]

  :commit
    (:required true
     :message "chore(wave26): archive router readiness artifacts"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Archived file count and groups."
     "Any intentionally missing report artifact."
     "Acceptance command results."])
