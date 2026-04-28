;; Wave 36 task-runner manifest.

(task-runner-manifest wave36-request-review-response-v0
  :schema "missiond.task-runner-manifest.v1"
  :wave wave36
  :brief_mode thin
  :shared_preamble_path ".missiond/claudecode/wave36-shared-preamble.md"
  :productive_only true
  :overlap_policy reject

  (node :task_id wave36-01-mission-request-review-response-v0
        :kind code-alignment
        :depends_on []
        :verification_tier local
        :dispatch_group A
        :estimated_minutes 60
        :heartbeat_minutes 10
        :write_scope ["crates/missiond-daemon/src/handlers/knowledge/request.rs"
                      "crates/missiond-mcp/src/tools/knowledge/request.rs"
                      ".missiond/v3/missiond-blueprint.lisp"]))
