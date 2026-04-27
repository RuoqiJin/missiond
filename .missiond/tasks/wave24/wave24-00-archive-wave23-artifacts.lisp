;; Wave 24 task contract.

(task wave24-00-archive-wave23-artifacts
  :schema "missiond.task-contract.v1"
  :title "Archive Wave 23 artifacts"
  :kind archive
  :status ready
  :owner "claudecode"
  :depends-on ["wave23-08-lisp-backfill-wave23-status"]
  :dispatch-strategy "fresh-code-alignment"
  :session-trace-writable true
  :goal "Archive all untracked Wave 23 task contracts, rendered ClaudeCode briefs, reports, and shared-memory artifacts after Wave 23 code and Lisp commits are complete."

  :write-scope
    [".missiond/tasks/wave23/**"
     ".missiond/claudecode/wave23-*.md"]

  :must-not-touch
    ["crates/**"
     "scripts/**"
     ".missiond/v2/**"
     ".missiond/tasks/wave24/**"]

  :requirements
    ["Stage only Wave 23 artifacts currently untracked."
     "Do not modify artifact contents while archiving; commit verbatim."
     "Include reports/, shared-memory.lisp, and any wave23 task/rendered brief files still untracked."
     "Do not stage Wave 24 task files."]

  :acceptance
    ["node scripts/check-task-contract.mjs --all"
     "node scripts/check-task-memory.mjs .missiond/tasks/wave23/shared-memory.lisp"
     "node scripts/check-session-trace.mjs .missiond/tasks/wave23/session-trace.lisp"
     "git diff --cached --name-only"
     "git diff --check -- .missiond/tasks/wave23 .missiond/claudecode"]

  :commit
    (:required true
     :message "chore(wave23): archive task artifacts"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Archived file count and groups."
     "Any intentionally missing report/trace artifact."
     "Acceptance command results."])
