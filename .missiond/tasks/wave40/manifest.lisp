;; Wave 40 task-runner manifest.

(task-runner-manifest wave40-report-preservation-v0
  :schema "missiond.task-runner-manifest.v2"
  :wave wave40
  :brief_mode thin
  :shared_preamble_path ".missiond/claudecode/wave40-shared-preamble.md"
  :productive_only true
  :overlap_policy reject
  :description "Close the parent-hotfix report preservation gap exposed after wave39."
  :generated_at "2026-04-29T02:55:06Z"
  :generator "codex-parent"

  (node :task_id wave40-01-parent-hotfix-report-preservation-v0
        :kind code-alignment
        :depends_on []
        :hard_deps []
        :soft_refs []
        :verification_tier local
        :dispatch_group A
        :estimated_minutes 45
        :heartbeat_minutes 10
        :write_scope [".missiond/v3/missiond-blueprint.lisp"
                      ".missiond/tasks/schema/report-contract-v1.lisp"
                      "scripts/task-runner-finalize-report.mjs"
                      "scripts/task-runner-parent-hotfix.mjs"
                      "scripts/check-task-report.mjs"
                      "scripts/check-v3-task-lifecycle-isomorphism.mjs"
                      "scripts/verify-task-runner-batch.mjs"]))
