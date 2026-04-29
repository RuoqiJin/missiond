;; Wave 43 task-runner manifest.

(task-runner-manifest wave43-v3-request-live-ipc-smoke-v0
  :schema "missiond.task-runner-manifest.v2"
  :wave wave43
  :brief_mode thin
  :shared_preamble_path ".missiond/claudecode/wave43-shared-preamble.md"
  :productive_only true
  :overlap_policy reject
  :description "Upgrade the V3 request-flow smoke from static fixtures to an opt-in live IPC path that stops at the execution gate."
  :generated_at "2026-04-29T03:49:58Z"
  :generator "codex-parent"

  (node :task_id wave43-01-v3-request-live-ipc-smoke-v0
        :kind code-alignment
        :depends_on []
        :hard_deps []
        :soft_refs []
        :verification_tier smoke
        :dispatch_group A
        :estimated_minutes 55
        :heartbeat_minutes 10
        :write_scope ["scripts/check-v3-request-flow-smoke.mjs"
                      ".missiond/v3/missiond-blueprint.lisp"
                      "crates/missiond-daemon/src/handlers/knowledge/request.rs"
                      "crates/missiond-mcp/src/tools/knowledge/request.rs"]))
