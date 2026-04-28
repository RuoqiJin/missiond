;; Wave 35 task contract.

(task wave35-01-mission-request-review-packet-v0
  :schema "missiond.task-contract.v1"
  :title "mission_request review packet v0"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on []
  :dispatch-strategy "fresh-code-alignment"
  :verification-tier local
  :dispatch-group "A"
  :estimated-minutes 45
  :heartbeat-minutes 10
  :session-trace-writable true
  :context-atlas-path ".missiond/tasks/wave35/context-atlas.lisp"
  :pattern-card-path ".missiond/tasks/wave35/pattern-cards.lisp"
  :router-policy-path ".missiond/router/router-policy-v1.lisp"
  :router-backend-registry-path ".missiond/router/router-backend-registry-v1.lisp"
  :goal "Project the V3 unified-entry review contract into mission_request responses: when request-local intent-alignment.lisp or plan.lisp exists, mission_request must return a compact review packet that tells the caller what artifact should be shown to the human, what approval state it represents, and which next action is expected. This is an interface contract only; do not auto-approve or auto-dispatch work."

  :write-scope
    ["crates/missiond-daemon/src/handlers/knowledge/request.rs"
     "crates/missiond-mcp/src/tools/knowledge/request.rs"
     ".missiond/v3/missiond-blueprint.lisp"]

  :must-not-touch
    ["crates/missiond-daemon/src/handlers/knowledge/directive.rs"
     "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
     "crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"
     "crates/missiond-daemon/src/handlers/knowledge/file_artifacts.rs"
     "crates/missiond-daemon/src/handlers/mod.rs"
     "crates/missiond-mcp/src/tools/mod.rs"
     "scripts/**"
     "packages/**"
     ".missiond/v1/**"
     ".missiond/v2/**"
     ".missiond/research/**"
     ".missiond/tasks/schema/**"
     ".missiond/tasks/wave31/**"
     ".missiond/tasks/wave32/**"
     ".missiond/tasks/wave33/**"
     ".missiond/tasks/wave34/**"
     ".missiond/tasks/wave35/manifest.lisp"
     ".missiond/tasks/wave35/context-atlas.lisp"
     ".missiond/tasks/wave35/pattern-cards.lisp"
     ".missiond/tasks/wave35/wave35-*.lisp"
     ".missiond/claudecode/**"]

  :requirements
    ["Update .missiond/v3/missiond-blueprint.lisp first. Add a compact review-packet contract under mission_request / unified-entry stating that human_interactive mode surfaces intent-alignment for intent approval, then plan.lisp for plan approval; trusted_agent may still fold intent into plan only through existing policy gates."
     "Add a `review_packet` object to mission_request start/advance/status responses when request paths are known. It should be deterministic, compact, and safe to show in a UI or CLI without re-reading files. Suggested fields: state, artifact_kind, artifact_path, artifact_exists, artifact_preview, prompt, allowed_responses, next_action, execute_allowed."
     "Derive review_packet from request-local artifact existence and the latest projection result. If intent-alignment.lisp exists and plan.lisp does not, state should be awaiting_intent_approval. If plan.lisp exists, state should be awaiting_plan_approval unless execution has already been explicitly requested. If neither exists, state should remain received or intent_drafting depending on available local facts."
     "Do not implement automatic approval, automatic execution, DB migrations, or workstation dispatch. This wave is only the review surface projection for the unified entry contract."
     "Use safe byte truncation for artifact_preview so Chinese text never panics on UTF-8 boundaries."
     "Update the MCP schema/description to document review_packet. Preserve additionalProperties=true and existing action names."
     "Add focused unit tests in request.rs for pure review_packet derivation and UTF-8-safe preview. Avoid AppState construction."]

  :acceptance
    ["cargo test -p missiond-daemon handlers::knowledge::request::tests"
     "cargo test -p missiond-mcp test_directive_plan_workflow_surfaces_registered"
     "cargo check -p missiond-daemon"
     "cargo check -p missiond-mcp"
     "node scripts/check-lisp-blueprint-compression.mjs"
     "node scripts/check-architecture-lisp.mjs --no-structure .missiond/v3/missiond-blueprint.lisp"
     "perl -ne 'exit 1 if /\\x00/' crates/missiond-daemon/src/handlers/knowledge/request.rs crates/missiond-mcp/src/tools/knowledge/request.rs .missiond/v3/missiond-blueprint.lisp"
     "git diff --check -- crates/missiond-daemon/src/handlers/knowledge/request.rs crates/missiond-mcp/src/tools/knowledge/request.rs .missiond/v3/missiond-blueprint.lisp"]

  :commit
    (:required true
     :message "feat(request): surface mission request review packet"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "V3 review-packet contract added."
     "Response fields added and state derivation rules."
     "Why no auto-approval / auto-dispatch was added."
     "Acceptance command results."])
