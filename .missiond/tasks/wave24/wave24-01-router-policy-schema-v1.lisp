;; Wave 24 task contract.

(task wave24-01-router-policy-schema-v1
  :schema "missiond.task-contract.v1"
  :title "Router policy schema/checker v1"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave24-00-archive-wave23-artifacts"]
  :dispatch-strategy "fresh-code-alignment"
  :session-trace-writable true
  :goal "Add a machine-checkable router-policy Lisp schema and read-only checker so future backend recommendations are data contracts, not prose."

  :write-scope
    [".missiond/tasks/schema/router-policy-v1.lisp"
     ".missiond/router/router-policy-v1.lisp"
     "scripts/check-router-policy.mjs"]

  :must-not-touch
    ["crates/**"
     ".missiond/v2/**"
     ".missiond/tasks/wave23/**"
     ".missiond/tasks/wave24/wave24-*.lisp"
     ".missiond/claudecode/**"]

  :requirements
    ["Create router-policy-v1 Lisp schema with backend classes: claudecode, missiond-llm-router, deterministic-checker, patch-worker, verifier-worker."
     "Create a seed policy file under .missiond/router/router-policy-v1.lisp."
     "Add scripts/check-router-policy.mjs as read-only checker with --json and --dry-fixture."
     "Validate policy ids, backend enum, rule priority uniqueness, explicit non-goals, and no runtime-replacement claims."
     "Do not add runtime dispatch integration in this task."]

  :acceptance
    ["node scripts/check-router-policy.mjs --dry-fixture"
     "node scripts/check-router-policy.mjs .missiond/router/router-policy-v1.lisp"
     "node scripts/check-task-contract.mjs --all"
     "git diff --check -- .missiond/tasks/schema/router-policy-v1.lisp .missiond/router/router-policy-v1.lisp scripts/check-router-policy.mjs"]

  :commit
    (:required true
     :message "feat(tasks): add router policy contract"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Schema fields and checker rules."
     "Dry fixtures added."
     "Acceptance command results."])
