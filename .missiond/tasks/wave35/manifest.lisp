;; Wave 35 task-runner manifest.

(task-runner-manifest wave35-request-review-packet-v0
  :schema "missiond.task-runner-manifest.v1"
  :wave wave35
  :brief_mode thin
  :shared_preamble_path ".missiond/claudecode/wave35-shared-preamble.md"
  :productive_only true
  :overlap_policy reject

  (node :task_id wave35-01-mission-request-review-packet-v0
        :kind code-alignment
        :depends_on []
        :verification_tier local
        :dispatch_group A
        :estimated_minutes 45
        :heartbeat_minutes 10
        :write_scope ["crates/missiond-daemon/src/handlers/knowledge/request.rs"
                      "crates/missiond-mcp/src/tools/knowledge/request.rs"
                      ".missiond/v3/missiond-blueprint.lisp"]))
