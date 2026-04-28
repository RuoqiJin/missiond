;; Wave 29 task contract.

(task wave29-06-ready-queue-planner-v0
  :schema "missiond.task-contract.v1"
  :title "Ready queue planner v0"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on ["wave29-01-context-atlas-schema-v0"
               "wave29-02-pattern-card-schema-v0"]
  :dispatch-strategy "fresh-code-alignment"
  :verification-tier local
  :dispatch-group "B"
  :estimated-minutes 45
  :heartbeat-minutes 10
  :session-trace-writable true
  :context-atlas-path ".missiond/tasks/wave29/context-atlas.lisp"
  :pattern-card-path ".missiond/tasks/wave29/pattern-cards.lisp"
  :router-policy-path ".missiond/router/router-policy-v1.lisp"
  :router-backend-registry-path ".missiond/router/router-backend-registry-v1.lisp"
  :goal "Evolve the read-only plan CLI from strict group-barrier batches to an additive ready-queue/phase-barrier planner. The new view should show which tasks can start as soon as dependency edges are satisfied, while preserving the current group-barrier output for backward compatibility."

  :write-scope
    ["scripts/plan-task-runner.mjs"
     "scripts/check-task-runner-manifest.mjs"
     ".missiond/tasks/schema/task-runner-manifest-v1.lisp"]

  :must-not-touch
    ["crates/**"
     ".missiond/v2/**"
     ".missiond/router/**"
     ".missiond/tasks/schema/task-contract-v1.lisp"
     ".missiond/tasks/schema/report-contract-v1.lisp"
     ".missiond/tasks/schema/context-atlas-v1.lisp"
     ".missiond/tasks/schema/pattern-card-v1.lisp"
     ".missiond/tasks/wave28/**"
     ".missiond/tasks/wave29/wave29-*.lisp"
     ".missiond/tasks/wave29/manifest.lisp"
     ".missiond/tasks/wave29/dispatch-plan.lisp"
     ".missiond/claudecode/**"
     "scripts/check-context-atlas.mjs"
     "scripts/check-pattern-card.mjs"
     "scripts/check-task-report.mjs"
     "scripts/verify-task-run.mjs"
     "scripts/verify-task-runner-batch.mjs"
     "scripts/prepare-task-runner-wave.mjs"
     "scripts/render-wave-briefs.mjs"]

  :requirements
    ["Keep existing group-barrier batches byte-compatible by default, or gate new output behind an explicit --schedule ready-queue flag. Do not break wave28 dry fixtures."
     "Add ready-queue output that releases a node when all dependency edges are satisfied, independent of unrelated long-running peers in the same dispatch group."
     "Priority should be deterministic and useful: critical-path or estimated-minutes first, then task id as tie breaker. Output should expose idle-window savings versus group-barrier where computable."
     "Preserve overlap safety: nodes with same dispatch_group write-scope overlap cannot be in the same ready window under reject policy."
     "Update task-runner-manifest schema docs/checker fixtures only as needed for additive planner metadata; do not change existing required node fields."
     "No dispatch, no spawn, no git mutation, no network, no LLM. Planner remains read-only."
     "Ensure source contains no raw NUL bytes so rg/grep keep treating it as text."]

  :acceptance
    ["node scripts/plan-task-runner.mjs --dry-fixture"
     "node scripts/check-task-runner-manifest.mjs --dry-fixture"
     "node scripts/check-task-contract.mjs --all"
     "perl -ne 'exit 1 if /\\x00/' scripts/plan-task-runner.mjs"
     "git diff --check -- scripts/plan-task-runner.mjs scripts/check-task-runner-manifest.mjs .missiond/tasks/schema/task-runner-manifest-v1.lisp"]

  :commit
    (:required true
     :message "feat(tasks): plan runner ready queue"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Ready-queue output fields."
     "Backward compatibility strategy."
     "Acceptance command results."])
