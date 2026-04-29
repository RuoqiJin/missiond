;; Wave 43 dispatch-time context atlas.

(context-atlas wave43-v3-request-live-ipc-smoke-v0
  :schema "missiond.context-atlas.dispatch.v0"
  :wave wave43
  :goal "Add an opt-in live IPC smoke for the V3 mission_request flow, stopping at awaiting_execution."
  :read-order [".missiond/claudecode/wave43-shared-preamble.md"
               ".missiond/tasks/wave43/context-atlas.lisp"
               ".missiond/tasks/wave43/pattern-cards.lisp"
               ".missiond/tasks/wave43/wave43-01-v3-request-live-ipc-smoke-v0.lisp"
               "scripts/check-v3-request-flow-smoke.mjs"
               ".missiond/v3/missiond-blueprint.lisp"
               "crates/missiond-daemon/src/handlers/knowledge/request.rs"
               "crates/missiond-mcp/src/tools/knowledge/request.rs"
               "scripts/task-runner-submit-dispatch.mjs"]

  (global-anchors
    (file "scripts/check-v3-request-flow-smoke.mjs"
      :purpose "Wave42 static+fixture request-flow checker. Extend it with a real --live-ipc mode without changing default behavior."
      :grep ["--live-ipc"
             "no real verification"
             "runFixtures"
             "derivePacket"
             "classifyReviewState"])
    (file "scripts/task-runner-submit-dispatch.mjs"
      :purpose "Existing IPC helper export. Reuse callToolViaIpc instead of duplicating socket code."
      :grep ["export function callToolViaIpc"
             "defaultEndpoint"
             "tools/call"])
    (file ".missiond/v3/missiond-blueprint.lisp"
      :purpose "V3 authority for the two-gate human flow and execution-gate non-goal."
      :grep ["review-response"
             "approve-intent"
             "approve-plan"
             "execute-plan"
             "awaiting_execution"
             "Do not let clients bypass plan-runner"])
    (file "crates/missiond-daemon/src/handlers/knowledge/request.rs"
      :purpose "Live path implementation. Only edit if the live IPC smoke exposes real drift from V3."
      :grep ["action_start"
             "action_respond"
             "ApproveIntent"
             "ApprovePlan"
             "materialize_request_plan_if_needed"
             "execute_plan requires execute=true"])
    (file "crates/missiond-mcp/src/tools/knowledge/request.rs"
      :purpose "MCP argument surface for live mission_request calls."
      :grep ["request_id"
             "compiler_mode"
             "persist"
             "approve_intent"
             "approve_plan"
             "execute_plan"])))
