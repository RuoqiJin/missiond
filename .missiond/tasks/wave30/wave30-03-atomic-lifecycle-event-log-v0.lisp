;; Wave 30 task contract.

(task wave30-03-atomic-lifecycle-event-log-v0
  :schema "missiond.task-contract.v1"
  :title "Atomic lifecycle event log v0"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on []
  :dispatch-strategy "fresh-code-alignment"
  :verification-tier local
  :dispatch-group "A"
  :estimated-minutes 45
  :heartbeat-minutes 10
  :session-trace-writable true
  :context-atlas-path ".missiond/tasks/wave30/context-atlas.lisp"
  :pattern-card-path ".missiond/tasks/wave30/pattern-cards.lisp"
  :router-policy-path ".missiond/router/router-policy-v1.lisp"
  :router-backend-registry-path ".missiond/router/router-backend-registry-v1.lisp"
  :goal "Replace direct max-seq + Edit writes for shared lifecycle facts with an orchestrator-owned append helper and schema. The new event log should be the future source for claims, trace starts, worker commits, parent hotfixes, finalization, receipts, and completions while projecting back to current shared-memory/session-trace files during migration."

  :write-scope
    [".missiond/tasks/schema/task-lifecycle-event-v1.lisp"
     "scripts/task-runner-append-event.mjs"
     "scripts/check-task-lifecycle-events.mjs"
     "scripts/project-task-lifecycle-ledger.mjs"
     "scripts/prepare-task-runner-wave.mjs"]

  :must-not-touch
    ["crates/**"
     ".missiond/v2/**"
     ".missiond/research/**"
     ".missiond/router/**"
     ".missiond/tasks/schema/task-contract-v1.lisp"
     ".missiond/tasks/schema/report-contract-v1.lisp"
     ".missiond/tasks/schema/task-runner-manifest-v1.lisp"
     ".missiond/tasks/schema/task-runner-manifest-v2.lisp"
     ".missiond/tasks/schema/verification-receipt-v1.lisp"
     ".missiond/tasks/wave28/**"
     ".missiond/tasks/wave29/**"
     ".missiond/tasks/wave30/wave30-*.lisp"
     ".missiond/tasks/wave30/manifest.lisp"
     ".missiond/tasks/wave30/dispatch-plan.lisp"
     ".missiond/claudecode/**"
     "scripts/task-runner-finalize-report.mjs"
     "scripts/task-runner-parent-hotfix.mjs"
     "scripts/check-staged-source-hygiene.mjs"
     "scripts/check-task-report.mjs"
     "scripts/check-verification-receipt.mjs"
     "scripts/verify-task-runner-batch.mjs"
     "scripts/plan-task-runner.mjs"
     "scripts/check-task-runner-manifest.mjs"
     "scripts/render-wave-briefs.mjs"]

  :requirements
    ["Add task-lifecycle-event-v1.lisp documenting event kinds for claim, trace_start, read, worker_commit, parent_hotfix, finalized_report, receipt, completion, and issue."
     "Create check-task-lifecycle-events.mjs with --dry-fixture and named exports. Validate repo-relative paths, unique ids, monotonic seq, known event kinds, commit hash format, and task id shape."
     "Create task-runner-append-event.mjs as the single append helper. It should avoid hand-edited max-seq races as much as possible in a file-based implementation and clearly document concurrency limits."
     "Create project-task-lifecycle-ledger.mjs to project event logs into current shared-memory/session-trace compatible facts during migration."
     "Update prepare-task-runner-wave.mjs to use the append/projection helpers for bootstrap events when possible, while preserving existing CLI behavior and dry-fixture output shape unless explicitly versioned."]

  :acceptance
    ["node scripts/check-task-lifecycle-events.mjs --dry-fixture"
     "node scripts/task-runner-append-event.mjs --dry-fixture"
     "node scripts/project-task-lifecycle-ledger.mjs --dry-fixture"
     "node scripts/prepare-task-runner-wave.mjs --dry-fixture"
     "node scripts/check-task-contract.mjs --all"
     "perl -ne 'exit 1 if /\\x00/' scripts/task-runner-append-event.mjs scripts/check-task-lifecycle-events.mjs scripts/project-task-lifecycle-ledger.mjs scripts/prepare-task-runner-wave.mjs"
     "git diff --check -- .missiond/tasks/schema/task-lifecycle-event-v1.lisp scripts/task-runner-append-event.mjs scripts/check-task-lifecycle-events.mjs scripts/project-task-lifecycle-ledger.mjs scripts/prepare-task-runner-wave.mjs"]

  :commit
    (:required true
     :message "feat(tasks): add lifecycle event log"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Event schema and kinds."
     "Append helper concurrency boundary."
     "Projection behavior for shared-memory/session-trace."
     "Acceptance command results."])

