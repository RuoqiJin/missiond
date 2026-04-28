;; Wave 29 task-runner manifest.

(task-runner-manifest wave29-runner-efficiency-v1
  :schema "missiond.task-runner-manifest.v1"
  :wave wave29
  :brief_mode thin
  :shared_preamble_path ".missiond/claudecode/wave29-shared-preamble.md"
  :productive_only true
  :overlap_policy reject

  (node :task_id wave29-01-context-atlas-schema-v0
        :kind code-alignment
        :depends_on []
        :verification_tier local
        :dispatch_group A
        :estimated_minutes 35
        :heartbeat_minutes 10
        :write_scope [".missiond/tasks/schema/context-atlas-v1.lisp"
                      "scripts/check-context-atlas.mjs"])

  (node :task_id wave29-02-pattern-card-schema-v0
        :kind code-alignment
        :depends_on []
        :verification_tier local
        :dispatch_group A
        :estimated_minutes 35
        :heartbeat_minutes 10
        :write_scope [".missiond/tasks/schema/pattern-card-v1.lisp"
                      "scripts/check-pattern-card.mjs"
                      ".missiond/patterns/schema-checker.pattern.lisp"
                      ".missiond/patterns/node-cli-readonly.pattern.lisp"
                      ".missiond/patterns/report-lineage.pattern.lisp"
                      ".missiond/patterns/cross-layer-smoke.pattern.lisp"
                      ".missiond/patterns/large-file-navigation.pattern.lisp"])

  (node :task_id wave29-04-parent-hotfix-lineage-v1
        :kind code-alignment
        :depends_on []
        :verification_tier local
        :dispatch_group A
        :estimated_minutes 30
        :heartbeat_minutes 10
        :write_scope ["scripts/check-task-report.mjs"
                      "scripts/verify-task-run.mjs"
                      "scripts/verify-task-runner-batch.mjs"
                      ".missiond/tasks/schema/report-contract-v1.lisp"])

  (node :task_id wave29-03-runner-wave-prep-v0
        :kind code-alignment
        :depends_on [wave29-01-context-atlas-schema-v0
                     wave29-02-pattern-card-schema-v0]
        :verification_tier local
        :dispatch_group B
        :estimated_minutes 35
        :heartbeat_minutes 10
        :write_scope ["scripts/prepare-task-runner-wave.mjs"
                      "scripts/render-wave-briefs.mjs"])

  (node :task_id wave29-05-verification-receipt-schema-v0
        :kind code-alignment
        :depends_on [wave29-04-parent-hotfix-lineage-v1]
        :verification_tier local
        :dispatch_group B
        :estimated_minutes 40
        :heartbeat_minutes 10
        :write_scope [".missiond/tasks/schema/verification-receipt-v1.lisp"
                      "scripts/check-verification-receipt.mjs"
                      "scripts/verify-task-runner-batch.mjs"])

  (node :task_id wave29-06-ready-queue-planner-v0
        :kind code-alignment
        :depends_on [wave29-01-context-atlas-schema-v0
                     wave29-02-pattern-card-schema-v0]
        :verification_tier local
        :dispatch_group B
        :estimated_minutes 45
        :heartbeat_minutes 10
        :write_scope ["scripts/plan-task-runner.mjs"
                      "scripts/check-task-runner-manifest.mjs"
                      ".missiond/tasks/schema/task-runner-manifest-v1.lisp"])

  (node :task_id wave29-07-runner-efficiency-smoke-v1
        :kind smoke
        :depends_on [wave29-03-runner-wave-prep-v0
                     wave29-04-parent-hotfix-lineage-v1
                     wave29-05-verification-receipt-schema-v0
                     wave29-06-ready-queue-planner-v0]
        :verification_tier smoke
        :dispatch_group C
        :estimated_minutes 45
        :heartbeat_minutes 10
        :write_scope ["scripts/check-context-atlas.mjs"
                      "scripts/check-pattern-card.mjs"
                      "scripts/prepare-task-runner-wave.mjs"
                      "scripts/check-task-report.mjs"
                      "scripts/check-verification-receipt.mjs"
                      "scripts/plan-task-runner.mjs"
                      "scripts/render-wave-briefs.mjs"
                      "scripts/verify-task-runner-batch.mjs"]))
