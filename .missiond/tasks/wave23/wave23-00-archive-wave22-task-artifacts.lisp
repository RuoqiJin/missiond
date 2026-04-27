;; Wave 23 task contract.

(task wave23-00-archive-wave22-task-artifacts
  :schema "missiond.task-contract.v1"
  :title "Archive Wave 22 task artifacts"
  :kind docs
  :status ready
  :owner "claudecode"
  :depends-on []
  :dispatch-strategy "fresh-code-alignment"
  :goal "Commit the Wave 22 task contracts, rendered briefs, reports, and shared-memory ledger left untracked after Wave 22."

  :write-scope
    [".missiond/tasks/wave22/**"
     ".missiond/claudecode/wave22-*.md"]

  :must-not-touch
    ["crates/**"
     "scripts/**"
     ".missiond/v2/*.lisp"
     ".missiond/tasks/wave23/**"]

  :requirements
    ["Stage only Wave 22 artifacts."
     "Do not stage Wave 23 task contracts, trace, shared memory, or briefs."
     "Do not edit Wave 22 files unless git diff --check reports whitespace problems."
     "Before committing, run git diff --cached --name-only and confirm every path is inside this task :write-scope."]

  :acceptance
    ["node scripts/check-task-contract.mjs --all"
     "node scripts/check-task-memory.mjs .missiond/tasks/wave22/shared-memory.lisp"
     "git diff --check -- .missiond/tasks/wave22 .missiond/claudecode/wave22-*.md"]

  :commit
    (:required true
     :message "chore(wave22): archive task artifacts"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Number of Wave 22 task contracts archived."
     "Number of rendered briefs archived."
     "Number of reports archived."
     "Shared-memory entry count."])
