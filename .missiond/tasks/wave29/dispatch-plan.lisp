;; Wave 29 dispatch plan.
;; This is orchestration metadata, not a ClaudeCode worker task.

(dispatch-plan wave29
  :schema "missiond.dispatch-plan.v0"
  :policy "productive-only"
  :shared-preamble ".missiond/claudecode/wave29-shared-preamble.md"
  :brief-mode thin
  :archive-policy "orchestrator-owned; never a worker task"
  :lisp-backfill-policy "codex-owned after code tasks; never a worker task"
  :index-policy "manifest + dispatch-plan metadata replaces parallel-dispatch-index worker task"
  :dispatch-upgrades ["context-atlas-path rendered in thin briefs"
                      "pattern-card-path rendered in thin briefs"
                      "shared preamble read event requested in session trace"
                      "parent hotfix lineage is explicit rather than amend-based"
                      "ready-queue planning is additive and keeps group-barrier backward compatibility"]

  :nodes
    [(node wave29-01-context-atlas-schema-v0
       :group A
       :verification-tier local
       :estimated-minutes 35
       :write-scope [".missiond/tasks/schema/context-atlas-v1.lisp"
                     "scripts/check-context-atlas.mjs"])
     (node wave29-02-pattern-card-schema-v0
       :group A
       :verification-tier local
       :estimated-minutes 35
       :write-scope [".missiond/tasks/schema/pattern-card-v1.lisp"
                     "scripts/check-pattern-card.mjs"
                     ".missiond/patterns/schema-checker.pattern.lisp"
                     ".missiond/patterns/node-cli-readonly.pattern.lisp"
                     ".missiond/patterns/report-lineage.pattern.lisp"
                     ".missiond/patterns/cross-layer-smoke.pattern.lisp"
                     ".missiond/patterns/large-file-navigation.pattern.lisp"])
     (node wave29-04-parent-hotfix-lineage-v1
       :group A
       :verification-tier local
       :estimated-minutes 30
       :write-scope ["scripts/check-task-report.mjs"
                     "scripts/verify-task-run.mjs"
                     "scripts/verify-task-runner-batch.mjs"
                     ".missiond/tasks/schema/report-contract-v1.lisp"])
     (node wave29-03-runner-wave-prep-v0
       :group B
       :depends-on [wave29-01-context-atlas-schema-v0
                    wave29-02-pattern-card-schema-v0]
       :verification-tier local
       :estimated-minutes 35
       :write-scope ["scripts/prepare-task-runner-wave.mjs"
                     "scripts/render-wave-briefs.mjs"])
     (node wave29-05-verification-receipt-schema-v0
       :group B
       :depends-on [wave29-04-parent-hotfix-lineage-v1]
       :verification-tier local
       :estimated-minutes 40
       :write-scope [".missiond/tasks/schema/verification-receipt-v1.lisp"
                     "scripts/check-verification-receipt.mjs"
                     "scripts/verify-task-runner-batch.mjs"])
     (node wave29-06-ready-queue-planner-v0
       :group B
       :depends-on [wave29-01-context-atlas-schema-v0
                    wave29-02-pattern-card-schema-v0]
       :verification-tier local
       :estimated-minutes 45
       :write-scope ["scripts/plan-task-runner.mjs"
                     "scripts/check-task-runner-manifest.mjs"
                     ".missiond/tasks/schema/task-runner-manifest-v1.lisp"])
     (node wave29-07-runner-efficiency-smoke-v1
       :group C
       :depends-on [wave29-03-runner-wave-prep-v0
                    wave29-04-parent-hotfix-lineage-v1
                    wave29-05-verification-receipt-schema-v0
                    wave29-06-ready-queue-planner-v0]
       :verification-tier smoke
       :estimated-minutes 45
       :write-scope ["scripts/check-context-atlas.mjs"
                     "scripts/check-pattern-card.mjs"
                     "scripts/prepare-task-runner-wave.mjs"
                     "scripts/check-task-report.mjs"
                     "scripts/check-verification-receipt.mjs"
                     "scripts/plan-task-runner.mjs"
                     "scripts/render-wave-briefs.mjs"
                     "scripts/verify-task-runner-batch.mjs"])])
