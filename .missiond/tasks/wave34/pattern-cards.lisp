;; Wave 34 dispatch-time pattern cards.

(pattern-cards wave34-autopilot-completion-ownership
  :schema "missiond.pattern-cards.dispatch.v0"
  :wave wave34

  (card lisp-policy-before-runtime-fix
    :use-for [wave34-01-autopilot-complete-close-v0]
    :summary "If runtime behavior is a policy boundary, state it in V3 Lisp first and make Rust a projection."
    :recipe ["Add a compact execution-ownership rule under workstation-config."
             "Name the prompt owner and close owner for delegated BoardTasks."
             "Mention the projected helpers/flags in the workstation implementation-map note."
             "Keep the change as a partial projection; do not introduce a new Lisp parser in this task."]
    :known-good [".missiond/v3/missiond-blueprint.lisp"])

  (card no-dual-prompt-for-delegated-task
    :use-for [wave34-01-autopilot-complete-close-v0]
    :summary "A delegated BoardTask must not run once from compute_slot.initial_prompt and then again from Autopilot. That splits ownership and can strand the BoardTask."
    :recipe ["Keep direct compute_slot create behavior compatible by default."
             "Add an explicit create-time option or helper that disables the initial prompt for task_delegate auto-provisioning."
             "Have task_delegate pass that option when it provisions a slot for a BoardTask."
             "Add focused tests proving the delegated path suppresses the initial prompt while the direct path remains documented."]
    :known-good ["crates/missiond-daemon/src/handlers/compute/task_delegate.rs"
                 "crates/missiond-daemon/src/handlers/compute/compute_slot.rs"])

  (card close-owner-waits-on-complete
    :use-for [wave34-01-autopilot-complete-close-v0]
    :summary "Autopilot is the owner that waits on pty.send for delegated BoardTasks. Its done transition should follow the Complete result unless the task is already done or blocked."
    :recipe ["Re-check the slot dispatch guard around state.pty.send. The guard is per-slot; avoid a release-before-send window."
             "Keep the existing Done and Blocked CAS behavior."
             "Add summary/error notes only from the owner that receives the final pty.send result."
             "Tests can stay focused on pure helpers and guard intent; do not build AppState just to test DB effects."]
    :known-good ["crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
                 "crates/missiond-daemon/src/slot_dispatch.rs"]))
