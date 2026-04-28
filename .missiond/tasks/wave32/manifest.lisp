;; Wave 32 task-runner manifest.

(task-runner-manifest wave32-autopilot-stability
  :schema "missiond.task-runner-manifest.v1"
  :wave wave32
  :brief_mode thin
  :shared_preamble_path ".missiond/claudecode/wave32-shared-preamble.md"
  :productive_only true
  :overlap_policy reject

  (node :task_id wave32-01-autopilot-timeout-budget-v0
        :kind code-alignment
        :depends_on []
        :verification_tier local
        :dispatch_group A
        :estimated_minutes 35
        :heartbeat_minutes 10
        :write_scope ["crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
                      ".missiond/v3/missiond-blueprint.lisp"]))
