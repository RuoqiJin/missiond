;; Wave 21 task contract.

(task wave21-00-archive-wave20-task-artifacts
  :schema "missiond.task-contract.v1"
  :title "Archive Wave 20 task artifacts"
  :kind docs
  :status ready
  :owner "claudecode"
  :depends-on []
  :dispatch-strategy "fresh-code-alignment"
  :goal "Commit the Wave 20 task contracts, rendered briefs, reports, and shared-memory ledger left untracked after Wave 20."

  :write-scope
    [".missiond/tasks/wave20/**"
     ".missiond/claudecode/wave20-*.md"]

  :must-not-touch
    ["crates/**"
     "scripts/**"
     ".missiond/v2/*.lisp"
     ".missiond/tasks/wave21/**"]

  :requirements
    ["Stage only Wave 20 artifacts."
     "Do not stage Wave 21 task contracts or briefs."
     "Do not edit Wave 20 files unless git diff --check reports whitespace problems."
     "Before committing, run git diff --cached --name-only and confirm every path is inside this task :write-scope."]

  :acceptance
    ["node scripts/check-task-contract.mjs --all"
     "node scripts/check-task-memory.mjs .missiond/tasks/wave20/shared-memory.lisp"
     "git diff --check -- .missiond/tasks/wave20 .missiond/claudecode/wave20-*.md"]

  :commit
    (:required true
     :message "chore(wave20): archive task artifacts"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Number of task contracts archived."
     "Number of rendered briefs archived."
     "Number of reports archived."
     "Shared-memory entry count."])
