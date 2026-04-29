;; Wave 48 task-runner manifest.

(task-runner-manifest wave48-context-pack-parallel-investigation-v0
  :schema "missiond.task-runner-manifest.v2"
  :wave wave48
  :brief_mode thin
  :shared_preamble_path ".missiond/claudecode/wave48-shared-preamble.md"
  :productive_only true
  :overlap_policy warn
  :description "Use the new V3 context-pack append-only surface to run parallel investigation before code-shard implementation."
  :generated_at "2026-04-29T06:01:00Z"
  :generator "codex-parent"

  (node :task_id wave48-01-context-autopilot-restart-recovery-v0
        :kind code-alignment
        :depends_on []
        :hard_deps []
        :soft_refs []
        :verification_tier local
        :dispatch_group A
        :estimated_minutes 35
        :heartbeat_minutes 10
        :write_scope [".missiond/tasks/wave48/context-pack.lisp"
                      ".missiond/tasks/wave48/shared-memory.lisp"
                      ".missiond/tasks/wave48/session-trace.lisp"
                      ".missiond/tasks/wave48/reports/wave48-01-context-autopilot-restart-recovery-v0.report.lisp"])

  (node :task_id wave48-02-context-dispatch-shard-plan-v0
        :kind code-alignment
        :depends_on []
        :hard_deps []
        :soft_refs []
        :verification_tier local
        :dispatch_group A
        :estimated_minutes 35
        :heartbeat_minutes 10
        :write_scope [".missiond/tasks/wave48/context-pack.lisp"
                      ".missiond/tasks/wave48/shared-memory.lisp"
                      ".missiond/tasks/wave48/session-trace.lisp"
                      ".missiond/tasks/wave48/reports/wave48-02-context-dispatch-shard-plan-v0.report.lisp"]))
