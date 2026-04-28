;; Wave 26 task contract.

(task wave26-00-archive-wave25-artifacts
  :schema "missiond.task-contract.v1"
  :title "Archive Wave 25 artifacts"
  :kind archive
  :status ready
  :owner "claudecode"
  :depends-on ["wave25-06-lisp-backfill-router-measurement-status"]
  :dispatch-strategy "fresh-code-alignment"
  :session-trace-writable true
  :router-policy-path ".missiond/router/router-policy-v1.lisp"
  :goal "Archive all untracked Wave 25 task contracts, rendered ClaudeCode briefs, reports, shared-memory ledger, and session-trace ledger after Wave 25 code and Lisp commits are complete."

  :write-scope
    [".missiond/tasks/wave25/**"
     ".missiond/claudecode/wave25-*.md"]

  :must-not-touch
    ["crates/**"
     "scripts/**"
     ".missiond/v2/**"
     ".missiond/tasks/wave26/wave26-*.lisp"
     ".missiond/claudecode/wave26-*.md"]

  :requirements
    ["Stage only Wave 25 artifacts currently untracked."
     "Do not modify artifact contents while archiving; commit verbatim."
     "Include task contracts, rendered briefs, reports, shared-memory.lisp, and session-trace.lisp."
     "It is acceptable that Wave25 Codex-owned Lisp backfill and coordination-index tasks have no report; call that out explicitly if true."
     "Do not stage Wave 26 task files or briefs."]

  :acceptance
    ["node scripts/check-task-contract.mjs --all"
     "node scripts/check-task-memory.mjs .missiond/tasks/wave25/shared-memory.lisp"
     "node scripts/check-session-trace.mjs .missiond/tasks/wave25/session-trace.lisp"
     "git diff --cached --name-only"
     "git diff --check -- .missiond/tasks/wave25 .missiond/claudecode"]

  :commit
    (:required true
     :message "chore(wave25): archive router measurement artifacts"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Archived file count and groups."
     "Any intentionally missing report artifact."
     "Acceptance command results."])

