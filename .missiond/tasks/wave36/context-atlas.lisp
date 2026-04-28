;; Wave 36 dispatch-time context atlas.
;; Read-only guidance for the worker. Task contract remains the source of truth.

(context-atlas wave36-request-review-response-v0
  :schema "missiond.context-atlas.dispatch.v0"
  :wave wave36
  :goal "Make mission_request accept explicit review responses after returning review_packet, without bypassing directive/plan gates."
  :read-order [".missiond/claudecode/wave36-shared-preamble.md"
               ".missiond/tasks/wave36/context-atlas.lisp"
               ".missiond/tasks/wave36/pattern-cards.lisp"
               ".missiond/v3/missiond-blueprint.lisp"
               ".missiond/tasks/wave36/wave36-01-mission-request-review-response-v0.lisp"]

  (global-anchors
    (file ".missiond/v3/missiond-blueprint.lisp"
      :purpose "Define review-response as a Lisp-first continuation of the existing review-packet contract."
      :grep ["review-packet"
             "surface mission_request"
             "unified-entry"
             "response shape"])
    (file "crates/missiond-daemon/src/handlers/knowledge/request.rs"
      :purpose "mission_request start/advance/status wrapper. Add respond routing, pure response parsing, review event rendering, and blocked/adapter response helpers here."
      :grep ["match action.as_str()"
             "action_status"
             "wrap_pipeline_result"
             "derive_review_packet"
             "ReviewPacketInputs"
             "request_paths_for"
             "build_event_lisp"
             "run_pipeline"
             "mod tests"])
    (file "crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"
      :purpose "Reference only. Existing helper already knows how to advance from approved_directive_id or approved_plan_id; do not edit."
      :grep ["run_pipeline"
             "approved_directive_id"
             "approved_plan_id"
             "execute=true"
             "build_plan_compile_args"])
    (file "crates/missiond-daemon/src/handlers/knowledge/directive.rs"
      :purpose "Reference only. Existing mission_directive approve surface is the gate to call; do not edit."
      :grep ["async fn action_approve"
             "review_decision"
             "directive_approve"
             "status\": \"approved\""])
    (file "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
      :purpose "Reference only. Existing mission_plan execute/approval behavior remains authoritative; do not edit."
      :grep ["approved_plan_id"
             "execute"
             "review_decision"
             "plan_id"])
    (file "crates/missiond-mcp/src/tools/knowledge/request.rs"
      :purpose "Document action=respond and its response/decision fields without changing tool registration elsewhere."
      :grep ["mission_request"
             "enum"
             "additionalProperties"
             "review_packet"])))
