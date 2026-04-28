;; Wave 34 dispatch-time context atlas.
;; Read-only guidance for the worker. Task contract remains the source of truth.

(context-atlas wave34-autopilot-completion-ownership
  :schema "missiond.context-atlas.dispatch.v0"
  :wave wave34
  :goal "Project a V3 Lisp execution-ownership rule into task_delegate, compute_slot, and Autopilot so delegated BoardTasks have one task prompt and deterministic close."
  :read-order [".missiond/claudecode/wave34-shared-preamble.md"
               ".missiond/tasks/wave34/context-atlas.lisp"
               ".missiond/tasks/wave34/pattern-cards.lisp"
               ".missiond/v3/missiond-blueprint.lisp"
               ".missiond/tasks/wave34/wave34-01-autopilot-complete-close-v0.lisp"]

  (global-anchors
    (file ".missiond/v3/missiond-blueprint.lisp"
      :purpose "Add the policy first: delegated BoardTask execution has a single prompt owner and Autopilot owns closure unless the worker self-closes through attached board tools."
      :grep ["workstation-config"
             "prompt-tool-contract"
             "Autopilot pty.send budget"
             "surface workstation-config"])
    (file "crates/missiond-daemon/src/handlers/compute/task_delegate.rs"
      :purpose "task_delegate auto-provisions dynamic slots, then creates a BoardTask. It must not let compute_slot send the task objective as an initial prompt for this path."
      :grep ["auto_provision_slot"
             "mission_compute_slot"
             "objective"
             "provisioned_new_slot"
             "create_args"])
    (file "crates/missiond-daemon/src/handlers/compute/compute_slot.rs"
      :purpose "compute_slot currently forwards objective_owned as PTYSpawnOptions.initial_prompt, which makes the spawner fire-and-forget before Autopilot owns the BoardTask prompt."
      :grep ["objective_owned"
             "initial_prompt"
             "PTYSpawnOptions"
             "resolve_model_projection"
             "mod tests"])
    (file "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
      :purpose "Autopilot claims the BoardTask, sends the actual prompt with pty.send, and should mark done after Complete unless already self-closed or blocked."
      :grep ["state.pty.send"
             "slot_dispatch"
             "task completed"
             "task already done"
             "BoardTaskStatus::Blocked"])
    (file "crates/missiond-daemon/src/slot_dispatch.rs"
      :purpose "Read-only reference: dispatch guard is per-slot, so keeping it across a blocking pty.send prevents concurrent sends to the same slot without starving other slots."
      :grep ["Per-slot dispatch guard"
             "try_acquire_guard"
             "SlotAcquireGuard"])))
