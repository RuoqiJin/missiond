;; Wave 33 task-runner manifest.

(task-runner-manifest wave33-lisp-isomorphism-cleanup
  :schema "missiond.task-runner-manifest.v1"
  :wave wave33
  :brief_mode thin
  :shared_preamble_path ".missiond/claudecode/wave33-shared-preamble.md"
  :productive_only true
  :overlap_policy reject

  (node :task_id wave33-01-autopilot-prompt-contract-v0
        :kind code-alignment
        :depends_on []
        :verification_tier local
        :dispatch_group A
        :estimated_minutes 35
        :heartbeat_minutes 10
        :write_scope ["crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
                      ".missiond/v3/missiond-blueprint.lisp"]))
