;; Wave 30 task contract.

(task wave30-05-lifecycle-receipt-smoke-v0
  :schema "missiond.task-contract.v1"
  :title "Lifecycle receipt smoke v0"
  :kind smoke
  :status ready
  :owner "claudecode"
  :depends-on ["wave30-01-parent-hotfix-finalizer-v0"
               "wave30-02-staged-source-hygiene-v0"
               "wave30-03-atomic-lifecycle-event-log-v0"
               "wave30-04-manifest-hard-soft-deps-v2"]
  :dispatch-strategy "fresh-code-alignment"
  :verification-tier smoke
  :dispatch-group "C"
  :estimated-minutes 45
  :heartbeat-minutes 10
  :session-trace-writable true
  :context-atlas-path ".missiond/tasks/wave30/context-atlas.lisp"
  :pattern-card-path ".missiond/tasks/wave30/pattern-cards.lisp"
  :router-policy-path ".missiond/router/router-policy-v1.lisp"
  :router-backend-registry-path ".missiond/router/router-backend-registry-v1.lisp"
  :goal "Add the cross-layer regression that proves Wave30 is one lifecycle architecture, not five isolated helpers: staged source hygiene passes, lifecycle events append/project, parent hotfix finalization updates report lineage, receipts bind final commit/files/tier, ready-queue ignores soft references, and batch verification accepts the finalized truth."

  :write-scope
    ["scripts/task-runner-finalize-report.mjs"
     "scripts/task-runner-parent-hotfix.mjs"
     "scripts/task-runner-append-event.mjs"
     "scripts/check-task-lifecycle-events.mjs"
     "scripts/check-staged-source-hygiene.mjs"
     "scripts/check-verification-receipt.mjs"
     "scripts/verify-task-runner-batch.mjs"
     "scripts/plan-task-runner.mjs"]

  :must-not-touch
    ["crates/**"
     ".missiond/v2/**"
     ".missiond/research/**"
     ".missiond/router/**"
     ".missiond/tasks/schema/**"
     ".missiond/tasks/wave28/**"
     ".missiond/tasks/wave29/**"
     ".missiond/tasks/wave30/wave30-*.lisp"
     ".missiond/tasks/wave30/manifest.lisp"
     ".missiond/tasks/wave30/dispatch-plan.lisp"
     ".missiond/claudecode/**"
     "scripts/check-task-report.mjs"
     "scripts/check-task-runner-manifest.mjs"
     "scripts/render-wave-briefs.mjs"
     "scripts/prepare-task-runner-wave.mjs"
     "scripts/project-task-lifecycle-ledger.mjs"]

  :requirements
    ["Add layer-local smoke fixtures to the owning scripts instead of a single opaque shell test. Each failure should identify the nearest broken layer."
     "Add one synthetic Wave30 fixture that starts from a worker draft report, appends a parent hotfix event, projects a finalized report, validates staged source hygiene, validates a receipt for the final commit/files/tier, and passes batch verification."
     "Assert that worker draft commit remains visible as agent_commit_hash while commit_hash/final_commit_hash/verified_commit_hash point at the finalized commit."
     "Assert ready-queue output does not wait on soft_refs. The fixture should include at least one hard dependency and one unrelated soft reference."
     "Audit all touched scripts for raw NUL bytes before commit."]

  :acceptance
    ["node scripts/task-runner-finalize-report.mjs --dry-fixture"
     "node scripts/task-runner-parent-hotfix.mjs --dry-fixture"
     "node scripts/task-runner-append-event.mjs --dry-fixture"
     "node scripts/check-task-lifecycle-events.mjs --dry-fixture"
     "node scripts/check-staged-source-hygiene.mjs --dry-fixture"
     "node scripts/check-verification-receipt.mjs --dry-fixture"
     "node scripts/verify-task-runner-batch.mjs --dry-fixture"
     "node scripts/plan-task-runner.mjs --dry-fixture"
     "node scripts/check-task-contract.mjs --all"
     "perl -ne 'exit 1 if /\\x00/' scripts/task-runner-finalize-report.mjs scripts/task-runner-parent-hotfix.mjs scripts/task-runner-append-event.mjs scripts/check-task-lifecycle-events.mjs scripts/check-staged-source-hygiene.mjs scripts/check-verification-receipt.mjs scripts/verify-task-runner-batch.mjs scripts/plan-task-runner.mjs"
     "git diff --check -- scripts/task-runner-finalize-report.mjs scripts/task-runner-parent-hotfix.mjs scripts/task-runner-append-event.mjs scripts/check-task-lifecycle-events.mjs scripts/check-staged-source-hygiene.mjs scripts/check-verification-receipt.mjs scripts/verify-task-runner-batch.mjs scripts/plan-task-runner.mjs"]

  :commit
    (:required true
     :message "test(tasks): smoke lifecycle finalization"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Synthetic lifecycle fixture shape."
     "Layer-local fixture increments."
     "Receipt/finalized report/ready-queue invariants."
     "Acceptance command results."])

