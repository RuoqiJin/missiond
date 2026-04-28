;; Wave 30 task-runner manifest.

(task-runner-manifest wave30-lifecycle-finalization-v1
  :schema "missiond.task-runner-manifest.v1"
  :wave wave30
  :brief_mode thin
  :shared_preamble_path ".missiond/claudecode/wave30-shared-preamble.md"
  :productive_only true
  :overlap_policy reject

  (node :task_id wave30-02-staged-source-hygiene-v0
        :kind code-alignment
        :depends_on []
        :verification_tier local
        :dispatch_group A
        :estimated_minutes 35
        :heartbeat_minutes 10
        :write_scope ["scripts/check-staged-source-hygiene.mjs"
                      "scripts/check-missiond-hooks.mjs"
                      "scripts/install-missiond-hooks.mjs"
                      ".githooks/pre-commit"])

  (node :task_id wave30-03-atomic-lifecycle-event-log-v0
        :kind code-alignment
        :depends_on []
        :verification_tier local
        :dispatch_group A
        :estimated_minutes 45
        :heartbeat_minutes 10
        :write_scope [".missiond/tasks/schema/task-lifecycle-event-v1.lisp"
                      "scripts/task-runner-append-event.mjs"
                      "scripts/check-task-lifecycle-events.mjs"
                      "scripts/project-task-lifecycle-ledger.mjs"
                      "scripts/prepare-task-runner-wave.mjs"])

  (node :task_id wave30-01-parent-hotfix-finalizer-v0
        :kind code-alignment
        :depends_on [wave30-02-staged-source-hygiene-v0
                     wave30-03-atomic-lifecycle-event-log-v0]
        :verification_tier local
        :dispatch_group B
        :estimated_minutes 50
        :heartbeat_minutes 10
        :write_scope ["scripts/task-runner-finalize-report.mjs"
                      "scripts/task-runner-parent-hotfix.mjs"
                      "scripts/check-task-report.mjs"
                      "scripts/verify-task-runner-batch.mjs"
                      ".missiond/tasks/schema/report-contract-v1.lisp"])

  (node :task_id wave30-04-manifest-hard-soft-deps-v2
        :kind code-alignment
        :depends_on [wave30-02-staged-source-hygiene-v0]
        :verification_tier local
        :dispatch_group B
        :estimated_minutes 45
        :heartbeat_minutes 10
        :write_scope [".missiond/tasks/schema/task-runner-manifest-v2.lisp"
                      "scripts/check-task-runner-manifest.mjs"
                      "scripts/plan-task-runner.mjs"
                      "scripts/render-wave-briefs.mjs"])

  (node :task_id wave30-05-lifecycle-receipt-smoke-v0
        :kind smoke
        :depends_on [wave30-01-parent-hotfix-finalizer-v0
                     wave30-02-staged-source-hygiene-v0
                     wave30-03-atomic-lifecycle-event-log-v0
                     wave30-04-manifest-hard-soft-deps-v2]
        :verification_tier smoke
        :dispatch_group C
        :estimated_minutes 45
        :heartbeat_minutes 10
        :write_scope ["scripts/task-runner-finalize-report.mjs"
                      "scripts/task-runner-parent-hotfix.mjs"
                      "scripts/task-runner-append-event.mjs"
                      "scripts/check-task-lifecycle-events.mjs"
                      "scripts/check-staged-source-hygiene.mjs"
                      "scripts/check-verification-receipt.mjs"
                      "scripts/verify-task-runner-batch.mjs"
                      "scripts/plan-task-runner.mjs"]))
