;; Wave 47 task-runner manifest.

(task-runner-manifest wave47-v3-request-real-dispatch-smoke-v0
  :schema "missiond.task-runner-manifest.v2"
  :wave wave47
  :brief_mode thin
  :shared_preamble_path ".missiond/claudecode/wave47-shared-preamble.md"
  :productive_only true
  :overlap_policy reject
  :description "Make mission_request real execute_plan dispatch observable through an explicit opt-in smoke without adding it to default or aggregate gates."
  :generated_at "2026-04-29T05:11:25Z"
  :generator "codex-parent"

  (node :task_id wave47-01-request-real-dispatch-smoke-v0
        :kind code-alignment
        :depends_on []
        :hard_deps []
        :soft_refs []
        :verification_tier smoke
        :dispatch_group A
        :estimated_minutes 70
        :heartbeat_minutes 10
        :write_scope [".missiond/v3/missiond-blueprint.lisp"
                      "scripts/check-v3-request-flow-smoke.mjs"
                      "crates/missiond-daemon/src/handlers/knowledge/request.rs"
                      "crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"
                      "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
                      "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
                      "crates/missiond-mcp/src/tools/knowledge/request.rs"]))
