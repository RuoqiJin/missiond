;; Wave 36 task contract.

(task wave36-01-mission-request-review-response-v0
  :schema "missiond.task-contract.v1"
  :title "mission_request review response v0"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on []
  :dispatch-strategy "fresh-code-alignment"
  :verification-tier local
  :dispatch-group "A"
  :estimated-minutes 60
  :heartbeat-minutes 10
  :session-trace-writable true
  :context-atlas-path ".missiond/tasks/wave36/context-atlas.lisp"
  :pattern-card-path ".missiond/tasks/wave36/pattern-cards.lisp"
  :router-policy-path ".missiond/router/router-policy-v1.lisp"
  :router-backend-registry-path ".missiond/router/router-backend-registry-v1.lisp"
  :goal "Close the next gap in the user-facing unified entry loop: after mission_request returns a review_packet, callers should be able to send an explicit review response back to mission_request instead of knowing the internal mission_directive / mission_plan surfaces. Implement a narrow v0 adapter for approve/reject/question decisions while preserving the existing directive/plan gates and avoiding autonomous execution."

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
     ".missiond/tasks/wave35/**"
     ".missiond/tasks/wave36/manifest.lisp"
     ".missiond/tasks/wave36/context-atlas.lisp"
     ".missiond/tasks/wave36/pattern-cards.lisp"
     ".missiond/tasks/wave36/wave36-*.lisp"
     ".missiond/claudecode/**"]

  :requirements
    ["Update .missiond/v3/missiond-blueprint.lisp first. Extend the unified-entry / mission_request contract with a review-response adapter: callers may answer a review_packet with approve_intent, reject_intent, ask_question, approve_plan, reject_plan, or execute_plan only through mission_request."
     "Add `respond` to mission_request actions. Inputs should include request_id, response (or decision), optional note, optional board_task_id, optional execute, and the same project/cwd/target_project root-resolution fields as status. Preserve existing start/advance/status behavior and response shape."
     "For approve_intent, require a persisted directive id/ref from the latest pipeline artifacts or an explicit approved_directive_id/directive_id argument. The adapter may call the existing mission_directive approve surface and then the existing unified-entry advance path to produce plan.lisp. If the needed ref is missing, return a structured blocked response with next_action instead of guessing."
     "For approve_plan / execute_plan, require a persisted plan id/ref or explicit approved_plan_id/plan_id. approve_plan should not execute by default; execute_plan requires execute=true (or response=execute_plan) and must still route through the existing mission_plan execute path. Never spawn work directly."
     "For reject_intent, reject_plan, and ask_question, do not mutate directive/plan approval state. Return a structured recorded/blocked response and append a request-local review event Lisp file under .missiond/requests/<id>/events so the user decision is auditable."
     "Persist minimal request-local response events for every respond call using atomic file writes and monotonically increasing local event sequence. Keep the event shape Lisp-first and documented in the V3 blueprint."
     "Keep review_packet derivation pure. After respond, return the latest review_packet plus respond_result, inner approval/advance payloads when invoked, and clear next_action text."
     "Update the MCP schema/description to document action=respond and response/decision fields. Preserve additionalProperties=true."
     "Add focused unit tests for pure response parsing, event sequencing/path choice, blocked missing-ref responses, and no-execute-by-default behavior. Avoid AppState-heavy tests where pure helpers are enough."]

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
     :message "feat(request): accept mission request review responses"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "V3 review-response contract added."
     "respond action inputs and response shape."
     "Which paths still require persisted directive/plan refs."
     "Why execution remains explicitly gated."
     "Acceptance command results."])
