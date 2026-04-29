;; Wave 44 task-runner manifest.

(task-runner-manifest wave44-v3-request-local-artifact-roots-v0
  :schema "missiond.task-runner-manifest.v2"
  :wave wave44
  :brief_mode thin
  :shared_preamble_path ".missiond/claudecode/wave44-shared-preamble.md"
  :productive_only true
  :overlap_policy reject
  :description "Make mission_request's live V3 path request-local by default; legacy compatibility artifact roots must be explicit opt-in."
  :generated_at "2026-04-29T04:08:08Z"
  :generator "codex-parent"

  (node :task_id wave44-01-request-local-artifact-roots-v0
        :kind code-alignment
        :depends_on []
        :hard_deps []
        :soft_refs []
        :verification_tier smoke
        :dispatch_group A
        :estimated_minutes 60
        :heartbeat_minutes 10
        :write_scope ["scripts/check-v3-request-flow-smoke.mjs"
                      ".missiond/v3/missiond-blueprint.lisp"
                      "crates/missiond-daemon/src/handlers/knowledge/request.rs"
                      "crates/missiond-mcp/src/tools/knowledge/request.rs"]))
