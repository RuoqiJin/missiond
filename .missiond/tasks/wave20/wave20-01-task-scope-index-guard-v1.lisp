;; Wave 20 task contract.

(task wave20-01-task-scope-index-guard-v1
  :schema "missiond.task-contract.v1"
  :title "Task scope index guard v1"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave20-00-archive-wave19-task-contracts"]
  :dispatch-strategy "agent-team"
  :goal "Prevent the Wave 19 git-index pollution failure by adding a task-contract aware guard for staged files and an optional pre-commit hook entry point."

  :write-scope
    ["scripts/task-scope-guard.mjs"
     ".githooks/pre-commit"
     ".missiond/tasks/schema/task-contract-v1.lisp"]

  :must-not-touch
    ["crates/**"
     ".missiond/v2/*.lisp"
     ".missiond/tasks/wave19/**"
     ".missiond/claudecode/wave19-*.md"]

  :requirements
    ["Use agent-team if useful: 使用 agent-team提高效率."
     "Add scripts/task-scope-guard.mjs with --task <task.lisp>, --mode staged|commit, --json, and --dry-fixture."
     "In staged mode, read git diff --cached --name-only and fail if any staged path is outside :write-scope or inside :must-not-touch."
     "In commit mode, accept --commit <hash> and delegate or share logic with verify-task-contract where practical."
     "Add .githooks/pre-commit that runs the guard only when MISSIOND_TASK_CONTRACT is set; without that env var it should exit 0."
     "The guard must be read-only: no git add, commit, reset, checkout, stash, push, merge, or rebase."]

  :acceptance
    ["node scripts/task-scope-guard.mjs --dry-fixture"
     "node scripts/check-task-contract.mjs --all"
     "node scripts/check-architecture-lisp.mjs --all-v2"
     "git diff --check -- scripts/task-scope-guard.mjs .githooks/pre-commit .missiond/tasks/schema/task-contract-v1.lisp"]

  :commit
    (:required true
     :message "feat(tasks): guard staged files by task scope"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Guard CLI synopsis."
     "Hook activation rule."
     "Dry-fixture coverage."
     "Acceptance command results."])
