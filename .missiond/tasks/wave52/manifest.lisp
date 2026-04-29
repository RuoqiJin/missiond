;; Wave 52 task-runner manifest.

(task-runner-manifest wave52-contract-artifact-validation-v0
  :schema "missiond.task-runner-manifest.v2"
  :wave wave52
  :brief_mode thin
  :shared_preamble_path ".missiond/claudecode/wave52-shared-preamble.md"
  :productive_only true
  :description "Make task contract verification validate the Lisp artifacts a worker touched, closing the wave51 invalid session-trace gap."
  :generated_at "2026-04-29T09:12:00Z"
  :generator "codex-parent"

  (node :task_id wave52-01-contract-artifact-validation-v0
        :kind code-alignment
        :depends_on []
        :hard_deps []
        :soft_refs []
        :verification_tier local
        :dispatch_group A
        :estimated_minutes 55
        :heartbeat_minutes 10
        :write_scope ["scripts/verify-task-contract.mjs"
                      ".missiond/v3/missiond-blueprint.lisp"
                      "scripts/check-v3-task-lifecycle-isomorphism.mjs"
                      ".missiond/tasks/wave52/shared-memory.lisp"
                      ".missiond/tasks/wave52/session-trace.lisp"
                      ".missiond/tasks/wave52/reports/wave52-01-contract-artifact-validation-v0.report.lisp"]))
