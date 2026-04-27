;; Wave 22 task contract.

(task wave22-01-hooks-default-on-doctor-v2
  :schema "missiond.task-contract.v1"
  :title "Hooks default-on doctor v2"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave22-00-archive-wave21-task-artifacts"]
  :dispatch-strategy "fresh-code-alignment"
  :goal "Make repo-local hook installation a first-class default preflight expectation for MissionD task workflows, without silently changing global user config."

  :write-scope
    ["scripts/install-missiond-hooks.mjs"
     "scripts/check-missiond-hooks.mjs"
     "scripts/render-claudecode-task.mjs"
     ".missiond/tasks/schema/task-contract-v1.lisp"]

  :must-not-touch
    ["crates/**"
     ".githooks/pre-commit"
     ".missiond/v2/*.lisp"
     ".missiond/tasks/wave21/**"]

  :requirements
    ["Add a default-on doctor status: task briefs should treat unset/wrong core.hooksPath as a preflight problem with a concrete install command."
     "Do not mutate git config from the renderer or doctor; only install-missiond-hooks --install may run git config --local core.hooksPath .githooks."
     "Render hook doctor commands in commit-required task briefs before staged guard commands."
     "Add dry-fixture coverage for installed, unset, wrong path, and missing hook file states."
     "Keep all hook config repo-local; do not write global git config."]

  :acceptance
    ["node scripts/install-missiond-hooks.mjs --dry-fixture"
     "node scripts/check-missiond-hooks.mjs --json"
     "node scripts/check-task-contract.mjs --all"
     "node scripts/check-architecture-lisp.mjs --all-v2"
     "git diff --check -- scripts/install-missiond-hooks.mjs scripts/check-missiond-hooks.mjs scripts/render-claudecode-task.mjs .missiond/tasks/schema/task-contract-v1.lisp"]

  :commit
    (:required true
     :message "feat(tasks): surface hooksPath as default preflight"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Doctor status model."
     "Rendered brief changes."
     "Mutating command boundary."
     "Acceptance command results."])
