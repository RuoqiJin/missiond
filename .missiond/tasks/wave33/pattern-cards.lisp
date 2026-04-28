;; Wave 33 dispatch-time pattern cards.

(pattern-cards wave33-lisp-isomorphism-cleanup
  :schema "missiond.pattern-cards.dispatch.v0"
  :wave wave33

  (card prompt-contract-before-code
    :use-for [wave33-01-autopilot-prompt-contract-v0]
    :summary "Runtime prompt behavior is currently encoded as ad hoc Rust suffixes. That is policy, so it should be stated in V3 Lisp before code changes."
    :recipe ["Add a compact prompt/tool contract under .missiond/v3/missiond-blueprint.lisp workstation-config."
             "Keep the contract high-density: what is always shown, what is conditional, and who owns completion when tools are absent."
             "Then make Autopilot helpers project that contract and mention those helpers in the implementation-map note."
             "Do not add a new parser in this task; this is a code-aligned partial projection, not the final generated interpreter."]
    :known-good [".missiond/v3/missiond-blueprint.lisp"
                 "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"])

  (card objective-dedupe
    :use-for [wave33-01-autopilot-prompt-contract-v0]
    :summary "mission_task_delegate uses the objective as BoardTask title and also starts description with that objective, so Autopilot's title+description assembly repeats the same instruction."
    :recipe ["Extract prompt base assembly into a pure helper."
             "If description equals title or starts with title followed by blank lines, return description only."
             "If description is distinct, keep the current title blank-line description shape."
             "Add focused tests for duplicate and non-duplicate cases."]
    :known-good ["crates/missiond-daemon/src/handlers/compute/task_delegate.rs"
                 "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"])

  (card conditional-board-tools
    :use-for [wave33-01-autopilot-prompt-contract-v0]
    :summary "A ClaudeCode slot may not have MissionD board MCP tools attached even when the daemon does. The prompt must not require tools the worker cannot see."
    :recipe ["Always surface the Board Task ID for audit and context."
             "Phrase board self-close as conditional: if mission_board_update / mission_board_note_add are available, call them."
             "If those tools are absent, instruct the worker to return a concise final summary; Autopilot/orchestrator remains responsible for closing the board task."
             "Add a pure test proving the old unconditional must-call wording is gone."]
    :known-good ["crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"]))
