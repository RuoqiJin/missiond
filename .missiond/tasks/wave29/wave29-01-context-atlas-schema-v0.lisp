;; Wave 29 task contract.

(task wave29-01-context-atlas-schema-v0
  :schema "missiond.task-contract.v1"
  :title "Context atlas schema v0"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on []
  :dispatch-strategy "fresh-code-alignment"
  :verification-tier local
  :dispatch-group "A"
  :estimated-minutes 35
  :heartbeat-minutes 10
  :session-trace-writable true
  :context-atlas-path ".missiond/tasks/wave29/context-atlas.lisp"
  :pattern-card-path ".missiond/tasks/wave29/pattern-cards.lisp"
  :router-policy-path ".missiond/router/router-policy-v1.lisp"
  :router-backend-registry-path ".missiond/router/router-backend-registry-v1.lisp"
  :goal "Introduce a durable context-atlas schema and checker so future waves can give workers precise file anchors, grep keywords, and read-order guidance before broad repository search. The dispatch-time wave29 atlas is guidance; this task makes the format machine-checkable for future waves."

  :write-scope
    [".missiond/tasks/schema/context-atlas-v1.lisp"
     "scripts/check-context-atlas.mjs"]

  :must-not-touch
    ["crates/**"
     ".missiond/v2/**"
     ".missiond/router/**"
     ".missiond/tasks/wave28/**"
     ".missiond/tasks/wave29/wave29-*.lisp"
     ".missiond/tasks/wave29/manifest.lisp"
     ".missiond/tasks/wave29/dispatch-plan.lisp"
     ".missiond/tasks/wave29/pattern-cards.lisp"
     ".missiond/claudecode/**"
     "scripts/check-pattern-card.mjs"
     "scripts/prepare-task-runner-wave.mjs"
     "scripts/check-task-report.mjs"
     "scripts/verify-task-run.mjs"
     "scripts/verify-task-runner-batch.mjs"
     "scripts/plan-task-runner.mjs"]

  :requirements
    ["Define schema missiond.context-atlas.v1 in .missiond/tasks/schema/context-atlas-v1.lisp."
     "Atlas records id, schema, purpose, read_order, file entries, task_focus entries, grep anchors, and avoid notes. It is navigation metadata only; it must not be treated as a behavioral contract."
     "Checker must validate repo-relative paths, non-empty anchors, duplicate file entries, duplicate task_focus entries, path traversal rejection, and malformed read_order values."
     "Checker must support --json, --stdin, and --dry-fixture, use scripts/lib/missiond_lisp.mjs, and never shell out / call git / call network / call LLM."
     "Fixtures must include a valid atlas, duplicate file rejection, duplicate task_focus rejection, absolute path rejection, traversal rejection, empty grep anchor rejection, and multiple atlas forms."]

  :acceptance
    ["node scripts/check-context-atlas.mjs --dry-fixture"
     "node scripts/check-task-contract.mjs --all"
     "git diff --check -- .missiond/tasks/schema/context-atlas-v1.lisp scripts/check-context-atlas.mjs"]

  :commit
    (:required true
     :message "feat(tasks): add context atlas schema"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Schema head and required fields."
     "Fixture categories."
     "Acceptance command results."])
