;; Wave 19 task contract.

(task wave19-01-archive-wave18-task-docs
  :schema "missiond.task-contract.v1"
  :title "Archive Wave 18 task briefs"
  :kind docs
  :status ready
  :owner "claudecode"
  :depends-on []
  :dispatch-strategy "fresh-code-alignment"
  :goal "Commit the untracked Wave 18 task documents so the working tree starts Wave 19 from a clean baseline."

  :write-scope
    [".missiond/claudecode/wave18-*.md"]

  :must-not-touch
    ["crates/**"
     ".missiond/v2/*.lisp"
     ".missiond/tasks/**"
     "scripts/**"]

  :requirements
    ["Stage only the existing .missiond/claudecode/wave18-*.md task documents."
     "Do not edit their contents unless git diff --check reports a whitespace problem."
     "Do not stage Wave 19 task contracts or rendered Wave 19 briefs."
     "Leave code and architecture Lisp untouched."]

  :acceptance
    ["git diff --check -- .missiond/claudecode/wave18-*.md"
     "git status --short -- .missiond/claudecode/wave18-*.md"]

  :commit
    (:required true
     :message "chore(wave18): archive task briefs"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Number of Wave 18 files committed."
     "Any files intentionally left untracked."])
