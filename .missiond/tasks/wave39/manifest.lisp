;; Wave 39 task-runner manifest.

(task-runner-manifest wave39-task-scoped-event-files-v0
  :schema "missiond.task-runner-manifest.v1"
  :wave wave39
  :brief_mode thin
  :shared_preamble_path ".missiond/claudecode/wave39-shared-preamble.md"
  :productive_only true
  :overlap_policy reject

  (node :task_id wave39-01-task-scoped-lifecycle-event-files-v0
        :kind code-alignment
        :depends_on []
        :verification_tier local
        :dispatch_group A
        :estimated_minutes 55
        :heartbeat_minutes 10
        :write_scope [".missiond/v3/missiond-blueprint.lisp"
                      ".missiond/tasks/schema/task-lifecycle-event-v1.lisp"
                      "scripts/check-task-lifecycle-events.mjs"
                      "scripts/task-runner-append-event.mjs"
                      "scripts/task-runner-wave-state.mjs"
                      "scripts/task-runner-next-action.mjs"
                      "scripts/task-runner-dispatch.mjs"
                      "scripts/task-runner-submit-dispatch.mjs"
                      "scripts/check-v3-task-lifecycle-isomorphism.mjs"
                      "scripts/verify-task-runner-batch.mjs"]))
