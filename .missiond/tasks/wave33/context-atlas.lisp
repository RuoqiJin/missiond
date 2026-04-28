;; Wave 33 dispatch-time context atlas.
;; Read-only guidance for the worker. Task contract remains the source of truth.

(context-atlas wave33-lisp-isomorphism-cleanup
  :schema "missiond.context-atlas.dispatch.v0"
  :wave wave33
  :goal "Move Autopilot prompt/tool behavior into V3 Lisp workstation-config and make Rust project that contract."
  :read-order [".missiond/claudecode/wave33-shared-preamble.md"
               ".missiond/tasks/wave33/context-atlas.lisp"
               ".missiond/tasks/wave33/pattern-cards.lisp"
               ".missiond/v3/missiond-blueprint.lisp"
               ".missiond/tasks/wave33/wave33-01-autopilot-prompt-contract-v0.lisp"]

  (global-anchors
    (file ".missiond/v3/missiond-blueprint.lisp"
      :purpose "The prompt/tool behavior must be expressed here first; Rust should be described as a projection."
      :grep ["workstation-config"
             "implementation-map"
             "surface workstation-config"
             "Autopilot pty.send budget"])
    (file "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
      :purpose "Autopilot currently assembles title+description prompts and appends unconditional board MCP self-close instructions."
      :grep ["Build prompt: template"
             "Decision Engine help suffix"
             "Board Task ID"
             "derive_pty_timeout_secs"
             "mod tests"])
    (file "crates/missiond-daemon/src/handlers/compute/task_delegate.rs"
      :purpose "Read-only reference: task_delegate stores objective as title and starts description with the same objective/context."
      :grep ["let mut description = objective.to_string()"
             "BoardTaskCreate"
             "description: Some(description)"
             "title: title.clone()"])))
