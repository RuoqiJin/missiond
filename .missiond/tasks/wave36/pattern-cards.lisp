;; Wave 36 dispatch-time pattern cards.

(pattern-cards wave36-request-review-response-v0
  :schema "missiond.pattern-cards.dispatch.v0"
  :wave wave36

  (card lisp-contract-before-response-adapter
    :use-for [wave36-01-mission-request-review-response-v0]
    :summary "When adding a unified-entry continuation, state the Lisp contract before adding a Rust branch."
    :recipe ["Extend the existing review-packet contract with review-response states and event artifacts."
             "Make mission_request the adapter, not a new policy owner."
             "Keep directive/plan approval semantics delegated to existing surfaces."
             "Return blocked responses when refs are missing rather than fabricating IDs."]
    :known-good [".missiond/v3/missiond-blueprint.lisp"
                 "crates/missiond-daemon/src/handlers/knowledge/request.rs"])

  (card adapter-not-bypass
    :use-for [wave36-01-mission-request-review-response-v0]
    :summary "A unified entry adapter may call existing gates, but must not bypass them or directly dispatch workstation work."
    :recipe ["approve_intent routes through mission_directive approve before plan compile."
             "approve_plan records approval/next action without execute unless the caller explicitly requests execution."
             "execute_plan requires execute=true or response=execute_plan and routes through mission_plan execute."
             "reject / ask_question are request-local review events only."]
    :known-good ["crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"
                 "crates/missiond-daemon/src/handlers/knowledge/request.rs"])

  (card local-review-event-ledger
    :use-for [wave36-01-mission-request-review-response-v0]
    :summary "Human-visible review responses should leave request-local Lisp events even when they do not mutate DB approval state."
    :recipe ["Write under .missiond/requests/<id>/events."
             "Use the next numeric local event seq; avoid overwriting existing events unless overwrite was explicit."
             "Use atomic writes and UTF-8-safe summaries."
             "Return event_path in respond_result so UIs can show the audit trail."]
    :known-good ["crates/missiond-daemon/src/handlers/knowledge/request.rs"]))
