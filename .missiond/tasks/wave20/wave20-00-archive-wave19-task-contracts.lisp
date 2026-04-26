;; Wave 20 task contract.

(task wave20-00-archive-wave19-task-contracts
  :schema "missiond.task-contract.v1"
  :title "Archive Wave 19 task contracts and rendered briefs"
  :kind docs
  :status ready
  :owner "claudecode"
  :depends-on []
  :dispatch-strategy "fresh-code-alignment"
  :goal "Commit the Wave 19 Lisp task contracts and rendered ClaudeCode briefs that remained untracked after Wave 19."

  :write-scope
    [".missiond/tasks/wave19/wave19-*.lisp"
     ".missiond/claudecode/wave19-*.md"]

  :must-not-touch
    ["crates/**"
     "scripts/**"
     ".missiond/v2/*.lisp"
     ".missiond/tasks/wave20/**"]

  :requirements
    ["Stage only Wave 19 task contracts and rendered Wave 19 briefs."
     "Do not stage Wave 20 task contracts."
     "Do not edit Wave 19 files unless git diff --check reports whitespace problems."
     "Before committing, inspect git diff --cached --name-only and confirm every staged path matches this task :write-scope."]

  :acceptance
    ["node scripts/check-task-contract.mjs --all"
     "git diff --check -- .missiond/tasks/wave19 .missiond/claudecode/wave19-*.md"]

  :commit
    (:required true
     :message "chore(wave19): archive task contracts"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Number of Lisp contracts archived."
     "Number of Markdown briefs archived."
     "Staged-file scope check result."])
