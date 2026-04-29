;; Wave 49 task-runner manifest.

(task-runner-manifest wave49-restart-recovery-smoke-v0
  :schema "missiond.task-runner-manifest.v2"
  :wave wave49
  :brief_mode thin
  :shared_preamble_path ".missiond/claudecode/wave49-shared-preamble.md"
  :productive_only true
  :description "Implement the accepted wave48 Group B restart-recovery smoke shard."
  :generated_at "2026-04-29T06:30:00Z"
  :generator "codex-parent"

  (node :task_id wave49-01-request-flow-restart-recovery-smoke-v0
        :kind test-fix
        :depends_on []
        :hard_deps []
        :soft_refs []
        :verification_tier local
        :dispatch_group A
        :estimated_minutes 45
        :heartbeat_minutes 10
        :write_scope ["scripts/check-v3-request-flow-smoke.mjs"
                      ".missiond/tasks/wave49/shared-memory.lisp"
                      ".missiond/tasks/wave49/session-trace.lisp"
                      ".missiond/tasks/wave49/reports/wave49-01-request-flow-restart-recovery-smoke-v0.report.lisp"]))
