;; Wave 28 task contract.

(task wave28-01-task-runner-manifest-schema-v0
  :schema "missiond.task-contract.v1"
  :title "Task runner manifest schema v0"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on []
  :dispatch-strategy "fresh-code-alignment"
  :verification-tier local
  :dispatch-group "A"
  :estimated-minutes 30
  :heartbeat-minutes 10
  :session-trace-writable true
  :router-policy-path ".missiond/router/router-policy-v1.lisp"
  :router-backend-registry-path ".missiond/router/router-backend-registry-v1.lisp"
  :goal "Introduce a machine-checkable task-runner manifest schema and checker. The manifest describes a batch of task contracts, dependency groups, write-scope overlap policy, verification tiers, and shared preamble path. It is orchestration metadata for MissionD/Codex, not a worker task and not a runtime backend switch."

  :write-scope
    [".missiond/tasks/schema/task-runner-manifest-v1.lisp"
     "scripts/check-task-runner-manifest.mjs"]

  :must-not-touch
    ["crates/**"
     ".missiond/v2/**"
     ".missiond/router/**"
     ".missiond/tasks/wave27/**"
     ".missiond/tasks/wave28/wave28-*.lisp"
     ".missiond/tasks/wave28/dispatch-plan.lisp"
     ".missiond/claudecode/**"
     "scripts/render-claudecode-task.mjs"
     "scripts/check-task-contract.mjs"
     "scripts/plan-task-runner.mjs"
     "scripts/render-wave-briefs.mjs"
     "scripts/verify-task-runner-batch.mjs"]

  :requirements
    ["Define schema missiond.task-runner-manifest.v1 in .missiond/tasks/schema/task-runner-manifest-v1.lisp."
     "Manifest must record wave id, brief_mode, shared_preamble_path, productive_only boolean, nodes, dependencies, verification_tier, dispatch_group, estimated_minutes, heartbeat_minutes, and write_scope."
     "Checker must validate task ids, dependency references, duplicate nodes, enum values, positive integer minutes, repo-relative paths, and no worker-task entries for archive/backfill/index categories."
     "Checker must detect write-scope overlap among nodes in the same dispatch group and emit a warning or error according to manifest policy; default policy should reject same-file overlaps for productive tasks."
     "Checker must support --json, --stdin, and --dry-fixture, use scripts/lib/missiond_lisp.mjs, and never shell out / call git / call network / call LLM."
     "Fixtures must include valid productive-only manifest, duplicate node rejection, missing dependency rejection, invalid verification tier rejection, overlap rejection, positive heartbeat validation, archive/backfill/index worker rejection, and missing shared preamble warning."]

  :acceptance
    ["node scripts/check-task-runner-manifest.mjs --dry-fixture"
     "node scripts/check-task-contract.mjs --all"
     "git diff --check -- .missiond/tasks/schema/task-runner-manifest-v1.lisp scripts/check-task-runner-manifest.mjs"]

  :commit
    (:required true
     :message "feat(tasks): add task runner manifest schema"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Schema fields and fixture categories."
     "Overlap/default policy behavior."
     "Acceptance command results."])
