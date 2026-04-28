;; Wave 34 task-runner manifest.

(task-runner-manifest wave34-autopilot-completion-ownership
  :schema "missiond.task-runner-manifest.v1"
  :wave wave34
  :brief_mode thin
  :shared_preamble_path ".missiond/claudecode/wave34-shared-preamble.md"
  :productive_only true
  :overlap_policy reject

  (node :task_id wave34-01-autopilot-complete-close-v0
        :kind code-alignment
        :depends_on []
        :verification_tier local
        :dispatch_group A
        :estimated_minutes 40
        :heartbeat_minutes 10
        :write_scope ["crates/missiond-daemon/src/handlers/compute/task_delegate.rs"
                      "crates/missiond-daemon/src/handlers/compute/compute_slot.rs"
                      "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
                      ".missiond/v3/missiond-blueprint.lisp"]))
