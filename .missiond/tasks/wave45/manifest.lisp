;; Wave 45 task-runner manifest.

(task-runner-manifest wave45-v3-request-execute-dry-run-v0
  :schema "missiond.task-runner-manifest.v2"
  :wave wave45
  :brief_mode thin
  :shared_preamble_path ".missiond/claudecode/wave45-shared-preamble.md"
  :productive_only true
  :overlap_policy reject
  :description "Make mission_request execute_plan live dry-run a first-class request-local regression without consuming workstation slots."
  :generated_at "2026-04-29T04:30:35Z"
  :generator "codex-parent"

  (node :task_id wave45-01-request-execute-dry-run-smoke-v0
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
