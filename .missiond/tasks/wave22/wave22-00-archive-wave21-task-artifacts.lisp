;; Wave 22 task contract.

(task wave22-00-archive-wave21-task-artifacts
  :schema "missiond.task-contract.v1"
  :title "Archive Wave 21 task artifacts"
  :kind docs
  :status ready
  :owner "claudecode"
  :depends-on []
  :dispatch-strategy "fresh-code-alignment"
  :goal "Commit the Wave 21 task contracts, rendered briefs, reports, and shared-memory ledger left untracked after Wave 21."

  :write-scope
    [".missiond/tasks/wave21/**"
     ".missiond/claudecode/wave21-*.md"]

  :must-not-touch
    ["crates/**"
     "scripts/**"
     ".missiond/v2/*.lisp"
     ".missiond/tasks/wave22/**"]

  :requirements
    ["Stage only Wave 21 artifacts."
     "Do not stage Wave 22 task contracts or briefs."
     "Do not edit Wave 21 files unless git diff --check reports whitespace problems."
     "Before committing, run git diff --cached --name-only and confirm every path is inside this task :write-scope."]

  :acceptance
    ["node scripts/check-task-contract.mjs --all"
     "node scripts/check-task-memory.mjs .missiond/tasks/wave21/shared-memory.lisp"
     "git diff --check -- .missiond/tasks/wave21 .missiond/claudecode/wave21-*.md"]

  :commit
    (:required true
     :message "chore(wave21): archive task artifacts"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Number of Wave 21 task contracts archived."
     "Number of rendered briefs archived."
     "Number of reports archived."
     "Shared-memory entry count."])
