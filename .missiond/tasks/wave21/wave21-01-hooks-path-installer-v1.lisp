;; Wave 21 task contract.

(task wave21-01-hooks-path-installer-v1
  :schema "missiond.task-contract.v1"
  :title "MissionD hooksPath installer v1"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave21-00-archive-wave20-task-artifacts"]
  :dispatch-strategy "fresh-code-alignment"
  :goal "Move scoped task guardrails from optional knowledge to a repo-local, explicit installer/doctor flow for core.hooksPath .githooks."

  :write-scope
    ["scripts/install-missiond-hooks.mjs"
     "scripts/check-missiond-hooks.mjs"
     ".githooks/pre-commit"
     ".missiond/tasks/schema/task-contract-v1.lisp"]

  :must-not-touch
    ["crates/**"
     ".missiond/v2/*.lisp"
     ".missiond/tasks/wave20/**"
     ".missiond/claudecode/wave20-*.md"]

  :requirements
    ["Add scripts/install-missiond-hooks.mjs with --check, --install, --json, and --dry-fixture."
     "In --check mode, read git config --get core.hooksPath and report whether it equals .githooks."
     "In --install mode, run only git config core.hooksPath .githooks; do not mutate anything else."
     "Add scripts/check-missiond-hooks.mjs as a read-only doctor alias if that keeps CLI cleaner."
     "Ensure .githooks/pre-commit remains env-gated by MISSIOND_TASK_CONTRACT and delegates to task-scope-guard."
     "Do not enable hooks globally; repo-local only."]

  :acceptance
    ["node scripts/install-missiond-hooks.mjs --dry-fixture"
     "node scripts/install-missiond-hooks.mjs --check --json"
     "node scripts/check-task-contract.mjs --all"
     "node scripts/check-architecture-lisp.mjs --all-v2"
     "git diff --check -- scripts/install-missiond-hooks.mjs scripts/check-missiond-hooks.mjs .githooks/pre-commit .missiond/tasks/schema/task-contract-v1.lisp"]

  :commit
    (:required true
     :message "feat(tasks): add MissionD hooksPath installer"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Installer CLI synopsis."
     "Mutating command proof."
     "Hook doctor output."
     "Acceptance command results."])
