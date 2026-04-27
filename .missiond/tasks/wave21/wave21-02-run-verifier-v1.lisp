;; Wave 21 task contract.

(task wave21-02-run-verifier-v1
  :schema "missiond.task-contract.v1"
  :title "Task run verifier v1"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave21-00-archive-wave20-task-artifacts"]
  :dispatch-strategy "agent-team"
  :goal "Add one verifier that ties together task.lisp, report.lisp, shared-memory completion, and git commit scope into a single post-run proof."

  :write-scope
    ["scripts/verify-task-run.mjs"
     "scripts/verify-task-contract.mjs"
     "scripts/check-task-report.mjs"
     "scripts/check-task-memory.mjs"
     "scripts/lib/missiond_lisp.mjs"
     ".missiond/tasks/schema/report-contract-v1.lisp"
     ".missiond/tasks/schema/shared-memory-v1.lisp"]

  :must-not-touch
    ["crates/**"
     ".missiond/v2/*.lisp"
     ".githooks/pre-commit"
     ".missiond/tasks/wave20/**"]

  :requirements
    ["Use agent-team if useful: 使用 agent-team提高效率."
     "Add scripts/verify-task-run.mjs with --task <task.lisp>, --report <report.lisp>, --memory <shared-memory.lisp>, --commit <hash>, --json, and --dry-fixture."
     "Verify: task contract passes, report references the same task_id, report commit_hash equals commit, commit scope passes verify-task-contract, and shared-memory has a completion entry for the task."
     "Keep the verifier read-only: no git add, commit, reset, checkout, stash, push, merge, or rebase."
     "If shared-memory is absent, fail with a structured diagnostic unless --allow-missing-memory is explicitly provided."]

  :acceptance
    ["node scripts/verify-task-run.mjs --dry-fixture"
     "node scripts/check-task-contract.mjs --all"
     "node scripts/check-task-report.mjs --dry-fixture"
     "node scripts/check-task-memory.mjs --dry-fixture"
     "node scripts/check-architecture-lisp.mjs --all-v2"
     "git diff --check -- scripts/verify-task-run.mjs scripts/verify-task-contract.mjs scripts/check-task-report.mjs scripts/check-task-memory.mjs scripts/lib/missiond_lisp.mjs .missiond/tasks/schema/report-contract-v1.lisp .missiond/tasks/schema/shared-memory-v1.lisp"]

  :commit
    (:required true
     :message "feat(tasks): verify complete task runs"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Run verifier CLI synopsis."
     "Dry-fixture coverage."
     "Read-only git proof."
     "Acceptance command results."])
