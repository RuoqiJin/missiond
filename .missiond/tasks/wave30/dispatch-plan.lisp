;; Wave 30 dispatch plan.
;; This is orchestration metadata, not a ClaudeCode worker task.

(dispatch-plan wave30
  :schema "missiond.dispatch-plan.v0"
  :policy "productive-only"
  :shared-preamble ".missiond/claudecode/wave30-shared-preamble.md"
  :brief-mode thin
  :archive-policy "orchestrator-owned; never a worker task"
  :lisp-backfill-policy "codex-owned after code tasks; never a worker task"
  :index-policy "manifest + dispatch-plan metadata replaces parallel-dispatch-index worker task"
  :mainline "Close the lisp-driven MissionD execution loop: intent-alignment.lisp -> PLAN.lisp -> workflow.lisp -> task runner lifecycle facts."
  :dispatch-upgrades ["parent hotfixes become orchestrator lifecycle events"
                      "finalized reports are projected by runner-owned helpers"
                      "staged source hygiene runs before final report/commit handoff"
                      "lifecycle events are append-only and projected into legacy ledgers"
                      "hard dependencies drive ready-queue release; soft references only enrich briefs"
                      "receipts bind finalized commit + files + verification tier"]

  :nodes
    [(node wave30-02-staged-source-hygiene-v0
       :group A
       :verification-tier local
       :estimated-minutes 35
       :write-scope ["scripts/check-staged-source-hygiene.mjs"
                     "scripts/check-missiond-hooks.mjs"
                     "scripts/install-missiond-hooks.mjs"
                     ".githooks/pre-commit"])
     (node wave30-03-atomic-lifecycle-event-log-v0
       :group A
       :verification-tier local
       :estimated-minutes 45
       :write-scope [".missiond/tasks/schema/task-lifecycle-event-v1.lisp"
                     "scripts/task-runner-append-event.mjs"
                     "scripts/check-task-lifecycle-events.mjs"
                     "scripts/project-task-lifecycle-ledger.mjs"
                     "scripts/prepare-task-runner-wave.mjs"])
     (node wave30-01-parent-hotfix-finalizer-v0
       :group B
       :depends-on [wave30-02-staged-source-hygiene-v0
                    wave30-03-atomic-lifecycle-event-log-v0]
       :verification-tier local
       :estimated-minutes 50
       :write-scope ["scripts/task-runner-finalize-report.mjs"
                     "scripts/task-runner-parent-hotfix.mjs"
                     "scripts/check-task-report.mjs"
                     "scripts/verify-task-runner-batch.mjs"
                     ".missiond/tasks/schema/report-contract-v1.lisp"])
     (node wave30-04-manifest-hard-soft-deps-v2
       :group B
       :depends-on [wave30-02-staged-source-hygiene-v0]
       :verification-tier local
       :estimated-minutes 45
       :write-scope [".missiond/tasks/schema/task-runner-manifest-v2.lisp"
                     "scripts/check-task-runner-manifest.mjs"
                     "scripts/plan-task-runner.mjs"
                     "scripts/render-wave-briefs.mjs"])
     (node wave30-05-lifecycle-receipt-smoke-v0
       :group C
       :depends-on [wave30-01-parent-hotfix-finalizer-v0
                    wave30-02-staged-source-hygiene-v0
                    wave30-03-atomic-lifecycle-event-log-v0
                    wave30-04-manifest-hard-soft-deps-v2]
       :verification-tier smoke
       :estimated-minutes 45
       :write-scope ["scripts/task-runner-finalize-report.mjs"
                     "scripts/task-runner-parent-hotfix.mjs"
                     "scripts/task-runner-append-event.mjs"
                     "scripts/check-task-lifecycle-events.mjs"
                     "scripts/check-staged-source-hygiene.mjs"
                     "scripts/check-verification-receipt.mjs"
                     "scripts/verify-task-runner-batch.mjs"
                     "scripts/plan-task-runner.mjs"])])
