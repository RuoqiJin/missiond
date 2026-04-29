;; Wave 43 dispatch-time pattern cards.

(pattern-cards wave43-v3-request-live-ipc-smoke-v0
  :schema "missiond.pattern-cards.dispatch.v0"
  :wave wave43

  (card opt-in-live-ipc-only
    :use-for [wave43-01-v3-request-live-ipc-smoke-v0]
    :summary "Live daemon calls are valuable smoke coverage but must stay opt-in and outside the aggregate default."
    :recipe ["Keep node scripts/check-v3-request-flow-smoke.mjs unchanged in default static+fixture mode."
             "Implement --live-ipc as an explicit mode with endpoint/session-id/request-id flags."
             "Do not add live IPC to check-v3-code-isomorphism-complete; the aggregate gate must remain deterministic and daemon-free."
             "Print a clear JSON summary of every mission_request call and artifact path checked."]
    :known-good ["scripts/check-v3-request-flow-smoke.mjs"
                 "scripts/task-runner-submit-dispatch.mjs"])

  (card stop-before-execution
    :use-for [wave43-01-v3-request-live-ipc-smoke-v0]
    :summary "The live smoke should prove the execution gate, not consume a workstation slot."
    :recipe ["Drive mission_request start -> respond approve_intent -> respond approve_plan."
             "Assert the final review_packet state is awaiting_execution and allowed_responses includes execute_plan."
             "Do not call response=execute_plan in acceptance."
             "If a --confirm-execute flag exists, make it explicitly refuse workstation dispatch in this checker and point users to manual mission_request usage."]
    :known-good [".missiond/v3/missiond-blueprint.lisp"
                 "crates/missiond-daemon/src/handlers/knowledge/request.rs"])

  (card bounded-live-side-effects
    :use-for [wave43-01-v3-request-live-ipc-smoke-v0]
    :summary "Live smoke will create request-local artifacts and DB rows; keep filesystem artifacts bounded and cleanup-capable."
    :recipe ["Require or auto-generate a request_id with a wave43-live-ipc-smoke prefix."
             "Write request-local artifacts only under .missiond/requests/<request_id>."
             "Support --cleanup to remove only that request-local directory after validation."
             "Document that DB directive/plan/BoardTask rows may remain as live smoke audit records."]
    :known-good ["crates/missiond-daemon/src/handlers/knowledge/request.rs"
                 "scripts/check-verification-receipt.mjs"]))
