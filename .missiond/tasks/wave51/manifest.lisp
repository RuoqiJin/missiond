;; Wave 51 task-runner manifest.

(task-runner-manifest wave51-concurrent-slot-dispatch-v0
  :schema "missiond.task-runner-manifest.v2"
  :wave wave51
  :brief_mode thin
  :shared_preamble_path ".missiond/claudecode/wave51-shared-preamble.md"
  :productive_only true
  :description "Implement the accepted context-pack shard that makes Autopilot start pty.send work concurrently across different slots in the same dispatch tick."
  :generated_at "2026-04-29T09:25:00Z"
  :generator "codex-parent"

  (node :task_id wave51-01-autopilot-concurrent-slot-dispatch-v0
        :kind code-alignment
        :depends_on []
        :hard_deps []
        :soft_refs []
        :verification_tier local
        :dispatch_group A
        :estimated_minutes 55
        :heartbeat_minutes 10
        :write_scope ["crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
                      ".missiond/v3/missiond-blueprint.lisp"
                      "scripts/check-v3-workstation-config-isomorphism.mjs"
                      ".missiond/tasks/wave51/shared-memory.lisp"
                      ".missiond/tasks/wave51/session-trace.lisp"
                      ".missiond/tasks/wave51/reports/wave51-01-autopilot-concurrent-slot-dispatch-v0.report.lisp"]))
