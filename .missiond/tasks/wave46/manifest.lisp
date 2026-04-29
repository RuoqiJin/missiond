;; Wave 46 task-runner manifest.

(task-runner-manifest wave46-v3-internal-execute-dry-run-v0
  :schema "missiond.task-runner-manifest.v2"
  :wave wave46
  :brief_mode thin
  :shared_preamble_path ".missiond/claudecode/wave46-shared-preamble.md"
  :productive_only true
  :overlap_policy reject
  :description "Make mission_request --execute-dry-run exercise execute_mode=internal and workstation-dispatch dry-run substrate, not only bridge mode."
  :generated_at "2026-04-29T04:47:14Z"
  :generator "codex-parent"

  (node :task_id wave46-01-request-internal-execute-dry-run-v0
        :kind code-alignment
        :depends_on []
        :hard_deps []
        :soft_refs []
        :verification_tier smoke
        :dispatch_group A
        :estimated_minutes 60
        :heartbeat_minutes 10
        :write_scope [".missiond/v3/missiond-blueprint.lisp"
                      "scripts/check-v3-request-flow-smoke.mjs"
                      "crates/missiond-daemon/src/handlers/knowledge/request.rs"
                      "crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"
                      "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
                      "crates/missiond-mcp/src/tools/knowledge/request.rs"]))
