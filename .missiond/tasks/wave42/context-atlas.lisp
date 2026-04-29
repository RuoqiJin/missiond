;; Wave 42 dispatch-time context atlas.
;; Read-only guidance for the worker. Task contract remains the source of truth.

(context-atlas wave42-v3-request-flow-smoke-v0
  :schema "missiond.context-atlas.dispatch.v0"
  :wave wave42
  :goal "Prove the V3 unified user request path as an executable smoke contract."
  :read-order [".missiond/claudecode/wave42-shared-preamble.md"
               ".missiond/tasks/wave42/context-atlas.lisp"
               ".missiond/tasks/wave42/pattern-cards.lisp"
               ".missiond/tasks/wave42/wave42-01-v3-request-flow-smoke-v0.lisp"
               ".missiond/v3/missiond-blueprint.lisp"
               "scripts/check-v3-code-isomorphism-complete.mjs"
               "scripts/check-v3-request-lisp-isomorphism.mjs"
               "crates/missiond-daemon/src/handlers/knowledge/request.rs"
               "crates/missiond-mcp/src/tools/knowledge/request.rs"]

  (global-anchors
    (file ".missiond/v3/missiond-blueprint.lisp"
      :purpose "V3 authority for mission_request, review_packet, review_response, artifact contracts, and compression checks."
      :grep ["unified-entry"
             "review-packet"
             "review-response"
             "surface mission_request"
             "artifact intent-alignment"
             "artifact plan"
             "compression-contract"])
    (file "crates/missiond-daemon/src/handlers/knowledge/request.rs"
      :purpose "Implementation of request-local artifacts, review packet state derivation, response routing, and unit tests."
      :grep ["derive_review_packet"
             "ReviewState"
             "ApprovePlan"
             "ExecutePlan"
             "request-local plan.lisp"
             "build_review_event_lisp"
             "materialize_request_plan_if_needed"])
    (file "crates/missiond-mcp/src/tools/knowledge/request.rs"
      :purpose "MCP schema for mission_request. Keep user-facing args and descriptions isomorphic with the V3 contract."
      :grep ["approve_intent"
             "approve_plan"
             "execute_plan"
             "stamping :plan_id"
             "review_packet"])
    (file "scripts/check-v3-request-lisp-isomorphism.mjs"
      :purpose "Existing narrow mission_request static checker. The new smoke checker should complement, not replace, this."
      :grep ["review_packet"
             "artifact_paths"
             "plan_id/:version/:board_task_id"
             "buildFixture"])
    (file "scripts/check-v3-code-isomorphism-complete.mjs"
      :purpose "Aggregate V3 completion gate. Add the request-flow smoke without weakening existing per-surface checks."
      :grep ["PER_SURFACE_CHECKERS"
             "compression-contract"
             "runLiveCheckers"
             "dry-fixture"])
    (file "scripts/check-lisp-blueprint-compression.mjs"
      :purpose "Blueprint compression checker. Pin the new smoke command only if the blueprint declares it."
      :grep ["compression-contract"
             "check-v3-code-isomorphism-complete"
             "checks"])))
