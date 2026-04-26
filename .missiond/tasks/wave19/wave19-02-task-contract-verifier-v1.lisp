;; Wave 19 task contract.

(task wave19-02-task-contract-verifier-v1
  :schema "missiond.task-contract.v1"
  :title "Task contract verifier v1"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave19-00-machine-contract-pilot"]
  :dispatch-strategy "agent-team"
  :goal "Add a verifier that checks a task Lisp contract against git state and a completed commit, so task.lisp becomes an enforceable dispatch contract rather than only a rendered brief."

  :write-scope
    ["scripts/verify-task-contract.mjs"
     "scripts/check-task-contract.mjs"
     "scripts/lib/missiond_lisp.mjs"
     ".missiond/tasks/schema/task-contract-v1.lisp"]

  :must-not-touch
    ["crates/**"
     ".missiond/v2/*.lisp"
     ".missiond/claudecode/wave18-*.md"]

  :requirements
    ["Use agent-team if useful: 使用 agent-team提高效率."
     "Add scripts/verify-task-contract.mjs; it must read one task.lisp and verify commit hash, commit message, changed files subset of :write-scope when :scope-check is write-scope-only, and no changed file overlaps :must-not-touch."
     "Support --commit <hash>, --json, and --dry-fixture."
     "Keep the existing checker backward compatible; only extend it if needed for shared helper reuse."
     "Do not require a ClaudeCode report file in v1; report-contract is a separate task."
     "Verifier must be read-only except normal process stdout/stderr; no git add, commit, reset, checkout, stash, push, merge, or rebase."]

  :acceptance
    ["node scripts/check-task-contract.mjs --all"
     "node scripts/verify-task-contract.mjs --dry-fixture"
     "node scripts/check-architecture-lisp.mjs --all-v2"
     "git diff --check -- scripts/verify-task-contract.mjs scripts/check-task-contract.mjs scripts/lib/missiond_lisp.mjs .missiond/tasks/schema/task-contract-v1.lisp"]

  :commit
    (:required true
     :message "feat(tasks): verify task contracts against commits"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Verifier CLI synopsis."
     "Dry-fixture coverage list."
     "Acceptance command results."])
