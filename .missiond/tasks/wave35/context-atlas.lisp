;; Wave 35 dispatch-time context atlas.
;; Read-only guidance for the worker. Task contract remains the source of truth.

(context-atlas wave35-request-review-packet-v0
  :schema "missiond.context-atlas.dispatch.v0"
  :wave wave35
  :goal "Make mission_request return the review packet needed by the user-facing intent -> plan approval loop, while keeping execution gated."
  :read-order [".missiond/claudecode/wave35-shared-preamble.md"
               ".missiond/tasks/wave35/context-atlas.lisp"
               ".missiond/tasks/wave35/pattern-cards.lisp"
               ".missiond/v3/missiond-blueprint.lisp"
               ".missiond/tasks/wave35/wave35-01-mission-request-review-packet-v0.lisp"]

  (global-anchors
    (file ".missiond/v3/missiond-blueprint.lisp"
      :purpose "State the review-packet contract before touching Rust. The current mission_request surface is code-aligned-partial and already describes request-local projections."
      :grep ["artifact mission-request"
             "unified-entry"
             "surface mission_request"
             "review_gates"])
    (file "crates/missiond-daemon/src/handlers/knowledge/request.rs"
      :purpose "mission_request start/advance/status wrapper. Add pure review-packet derivation near RequestPaths / projection helpers, then include it in wrapper/status responses."
      :grep ["wrap_pipeline_result"
             "action_status"
             "RequestPaths"
             "build_artifact_existence"
             "run_projection"
             "safe_byte_truncate"
             "mod tests"])
    (file "crates/missiond-mcp/src/tools/knowledge/request.rs"
      :purpose "Tool description and JSON schema for mission_request. Document response review_packet without changing actions or compatibility posture."
      :grep ["mission_request"
             "Wrapper response shape"
             "additionalProperties"])
    (file "crates/missiond-core/src/util.rs"
      :purpose "Reference only: use the existing UTF-8-safe byte truncation helper for artifact previews."
      :grep ["safe_byte_truncate"])))
