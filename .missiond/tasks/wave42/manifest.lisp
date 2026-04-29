;; Wave 42 task-runner manifest.

(task-runner-manifest wave42-v3-request-flow-smoke-v0
  :schema "missiond.task-runner-manifest.v2"
  :wave wave42
  :brief_mode thin
  :shared_preamble_path ".missiond/claudecode/wave42-shared-preamble.md"
  :productive_only true
  :overlap_policy reject
  :description "Add an executable V3 request-flow smoke gate for the user-facing intent -> plan -> execute-review path."
  :generated_at "2026-04-29T03:30:31Z"
  :generator "codex-parent"

  (node :task_id wave42-01-v3-request-flow-smoke-v0
        :kind code-alignment
        :depends_on []
        :hard_deps []
        :soft_refs []
        :verification_tier smoke
        :dispatch_group A
        :estimated_minutes 50
        :heartbeat_minutes 10
        :write_scope [".missiond/v3/missiond-blueprint.lisp"
                      "scripts/check-v3-request-flow-smoke.mjs"
                      "scripts/check-v3-code-isomorphism-complete.mjs"
                      "scripts/check-lisp-blueprint-compression.mjs"
                      "crates/missiond-daemon/src/handlers/knowledge/request.rs"
                      "crates/missiond-mcp/src/tools/knowledge/request.rs"]))
