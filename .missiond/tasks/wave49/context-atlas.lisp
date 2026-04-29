;; Wave 49 dispatch-time context atlas.

(context-atlas wave49-restart-recovery-smoke-v0
  :schema "missiond.context-atlas.dispatch.v0"
  :wave wave49
  :goal "Implement the opt-in request-flow restart-recovery smoke accepted by wave48 integration-plan."
  :read-order [".missiond/claudecode/wave49-shared-preamble.md"
               ".missiond/tasks/wave49/context-atlas.lisp"
               ".missiond/tasks/wave49/pattern-cards.lisp"
               ".missiond/tasks/wave49/wave49-01-request-flow-restart-recovery-smoke-v0.lisp"
               ".missiond/tasks/wave48/context-pack.lisp"
               "scripts/check-v3-request-flow-smoke.mjs"
               "scripts/check-v3-code-isomorphism-complete.mjs"]

  (global-anchors
    (file ".missiond/tasks/wave48/context-pack.lisp"
      :purpose "Accepted shard and integration-plan authority for this task."
      :grep ["wave48-integration-plan-001"
             "wave48-02-shard-recovery-smoke"
             "restart-during-dispatch"])
    (file "scripts/check-v3-request-flow-smoke.mjs"
      :purpose "Only implementation file. Preserve existing CLI modes and add the opt-in restart smoke."
      :grep ["--execute-real-dispatch"
             "runLiveIpcSmoke"
             "executeRealDispatch"
             "dryFixture"
             "parseArgs"])
    (file "scripts/check-v3-code-isomorphism-complete.mjs"
      :purpose "Aggregate V3 gate that must still pass after the smoke script changes."
      :grep ["check-v3-request-flow-smoke.mjs"
             "request-flow"])))
