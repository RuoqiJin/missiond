;; Wave 38 task-runner manifest.

(task-runner-manifest wave38-workflow-methodology-artifact-v0
  :schema "missiond.task-runner-manifest.v1"
  :wave wave38
  :brief_mode thin
  :shared_preamble_path ".missiond/claudecode/wave38-shared-preamble.md"
  :productive_only true
  :overlap_policy reject

  (node :task_id wave38-01-workflow-methodology-artifact-v0
        :kind code-alignment
        :depends_on []
        :verification_tier local
        :dispatch_group A
        :estimated_minutes 50
        :heartbeat_minutes 10
        :write_scope [".missiond/v3/missiond-blueprint.lisp"
                      "scripts/check-v3-workflow-isomorphism.mjs"
                      "crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
                      "crates/missiond-mcp/src/tools/knowledge/workflow.rs"]))
