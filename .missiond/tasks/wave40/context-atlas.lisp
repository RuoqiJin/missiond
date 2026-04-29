;; Wave 40 dispatch-time context atlas.
;; Read-only guidance for the worker. Task contract remains the source of truth.

(context-atlas wave40-report-preservation-v0
  :schema "missiond.context-atlas.dispatch.v0"
  :wave wave40
  :goal "Make parent-hotfix finalization preserve worker report detail while adding finalized lineage."
  :read-order [".missiond/claudecode/wave40-shared-preamble.md"
               ".missiond/tasks/wave40/context-atlas.lisp"
               ".missiond/tasks/wave40/pattern-cards.lisp"
               ".missiond/tasks/wave40/wave40-01-parent-hotfix-report-preservation-v0.lisp"
               ".missiond/v3/missiond-blueprint.lisp"
               ".missiond/tasks/schema/report-contract-v1.lisp"
               "scripts/task-runner-finalize-report.mjs"
               "scripts/task-runner-parent-hotfix.mjs"]

  (global-anchors
    (file ".missiond/v3/missiond-blueprint.lisp"
      :purpose "V3 authority for lifecycle/report projection. The task-runner-cli note already mentions parent-hotfix finalization but does not yet pin preservation of worker report details."
      :grep ["policy parent-hotfix-finalization"
             "surface task-runner-cli"
             "scripts/task-runner-finalize-report.mjs"
             "scripts/task-runner-parent-hotfix.mjs"
             "parent-hotfix finalization"
             "final-report artifact"])
    (file ".missiond/tasks/schema/report-contract-v1.lisp"
      :purpose "Report contract schema/docs. Add the preservation invariant here before code changes."
      :grep ["parent hotfix"
             "agent_commit_hash"
             "final_commit_hash"
             "verified_commit_hash"
             "parent_patches"
             "acceptance_results"])
    (file "scripts/task-runner-finalize-report.mjs"
      :purpose "Current finalizer reconstructs a minimal report object; this is the main implementation target."
      :grep ["finalizeReportObject"
             "finalizeReportSource"
             "renderFinalReport"
             "acceptanceResults"
             "acceptanceCommands"
             "runFixtures"])
    (file "scripts/task-runner-parent-hotfix.mjs"
      :purpose "Parent hotfix helper that delegates to the finalizer. It must inherit the preservation behavior."
      :grep ["planParentHotfixFromSource"
             "finalizeReportSource"
             "acceptanceCommands"
             "runFixtures"])
    (file "scripts/verify-task-runner-batch.mjs"
      :purpose "Batch smoke surface. Add a preservation regression fixture without broad refactors."
      :grep ["task-runner-finalize-report"
             "task-runner-parent-hotfix"
             "parent_patches"
             "lineage"])
    (file "scripts/check-v3-task-lifecycle-isomorphism.mjs"
      :purpose "Cross-layer isomorphism checker. Pin Lisp wording and finalizer/helper preservation affordances."
      :grep ["parent-hotfix-finalization"
             "task-runner-finalize-report"
             "task-runner-parent-hotfix"
             "parent_patches"])))
