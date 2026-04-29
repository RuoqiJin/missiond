;; Wave 50 task-runner manifest.

(task-runner-manifest wave50-timeout-derived-lease-v0
  :schema "missiond.task-runner-manifest.v2"
  :wave wave50
  :brief_mode thin
  :shared_preamble_path ".missiond/claudecode/wave50-shared-preamble.md"
  :productive_only true
  :description "Implement the accepted context-pack shard that makes BoardTask claim leases project from timeout_secs instead of a fixed 20 minutes."
  :generated_at "2026-04-29T08:05:00Z"
  :generator "codex-parent"

  (node :task_id wave50-01-board-task-timeout-lease-v0
        :kind code-alignment
        :depends_on []
        :hard_deps []
        :soft_refs []
        :verification_tier local
        :dispatch_group A
        :estimated_minutes 45
        :heartbeat_minutes 10
        :write_scope ["crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
                      ".missiond/v3/missiond-blueprint.lisp"
                      "scripts/check-v3-workstation-config-isomorphism.mjs"
                      ".missiond/tasks/wave50/shared-memory.lisp"
                      ".missiond/tasks/wave50/session-trace.lisp"
                      ".missiond/tasks/wave50/reports/wave50-01-board-task-timeout-lease-v0.report.lisp"]))
