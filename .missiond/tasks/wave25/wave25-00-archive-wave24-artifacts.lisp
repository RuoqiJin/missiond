;; Wave 25 task contract.

(task wave25-00-archive-wave24-artifacts
  :schema "missiond.task-contract.v1"
  :title "Archive Wave 24 artifacts"
  :kind archive
  :status ready
  :owner "claudecode"
  :depends-on ["wave24-07-lisp-backfill-router-dry-run-status"]
  :dispatch-strategy "fresh-code-alignment"
  :session-trace-writable true
  :router-policy-path ".missiond/router/router-policy-v1.lisp"
  :goal "Archive all untracked Wave 24 task contracts, rendered ClaudeCode briefs, reports, shared-memory ledger, and session-trace ledger after Wave 24 code and Lisp commits are complete."

  :write-scope
    [".missiond/tasks/wave24/**"
     ".missiond/claudecode/wave24-*.md"]

  :must-not-touch
    ["crates/**"
     "scripts/**"
     ".missiond/v2/**"
     ".missiond/tasks/wave25/wave25-*.lisp"
     ".missiond/claudecode/wave25-*.md"]

  :requirements
    ["Stage only Wave 24 artifacts currently untracked."
     "Do not modify artifact contents while archiving; commit verbatim."
     "Include task contracts, rendered briefs, reports, shared-memory.lisp, and session-trace.lisp."
     "Do not stage Wave 25 task files or briefs."]

  :acceptance
    ["node scripts/check-task-contract.mjs --all"
     "node scripts/check-task-memory.mjs .missiond/tasks/wave24/shared-memory.lisp"
     "node scripts/check-session-trace.mjs .missiond/tasks/wave24/session-trace.lisp"
     "git diff --cached --name-only"
     "git diff --check -- .missiond/tasks/wave24 .missiond/claudecode"]

  :commit
    (:required true
     :message "chore(wave24): archive router dry-run artifacts"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Archived file count and groups."
     "Any intentionally missing report artifact."
     "Acceptance command results."])

