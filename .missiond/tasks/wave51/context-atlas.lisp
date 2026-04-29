;; Wave 51 dispatch-time context atlas.

(context-atlas wave51-concurrent-slot-dispatch-v0
  :schema "missiond.context-atlas.dispatch.v0"
  :wave wave51
  :goal "Make Autopilot start BoardTask pty.send calls concurrently across different slots within one dispatch tick."
  :read-order [".missiond/claudecode/wave51-shared-preamble.md"
               ".missiond/tasks/wave51/context-pack.lisp"
               ".missiond/tasks/wave51/wave51-01-autopilot-concurrent-slot-dispatch-v0.lisp"
               ".missiond/tasks/wave51/context-atlas.lisp"
               ".missiond/tasks/wave51/pattern-cards.lisp"
               "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
               "crates/missiond-daemon/src/slot_dispatch.rs"
               ".missiond/v3/missiond-blueprint.lisp"
               "scripts/check-v3-workstation-config-isomorphism.mjs"]

  (global-anchors
    (file ".missiond/tasks/wave51/context-pack.lisp"
      :purpose "Shard authority for this code worker."
      :grep ["wave51-integration-plan-001"
             "concurrent-slot-send"
             "(group :id A :shards [concurrent-slot-send])"])
    (file "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
      :purpose "Implementation target. dispatch_board_tasks currently loops tasks and awaits state.pty.send inline; close-owner and KB feedback logic live in the post-send branch."
      :grep ["pub(crate) async fn dispatch_board_tasks"
             "let mut dispatched_slots: HashSet<String>"
             "state.slot_dispatch.try_acquire_guard(&slot_id)"
             "state.pty.send(&slot_id, &full_prompt, timeout_ms).await"
             "decide_close_action(current_status)"])
    (file "crates/missiond-daemon/src/slot_dispatch.rs"
      :purpose "Per-slot guard semantics. Same-slot exclusion is correct; different slots should remain independent."
      :grep ["pub(crate) struct SlotDispatchGuard"
             "pub fn try_acquire_guard"
             "test_independent_slots"])
    (file ".missiond/v3/missiond-blueprint.lisp"
      :purpose "V3 workstation-config invariant text."
      :grep ["The per-slot dispatch guard MUST be held across the entire state.pty.send call"
             "does not starve callers targeting other slots"
             "Autopilot pty.send budget"])
    (file "scripts/check-v3-workstation-config-isomorphism.mjs"
      :purpose "Source checker that pins blueprint invariant strings into Rust source."
      :grep ["state.slot_dispatch.try_acquire_guard(&slot_id)"
             "state.pty.send(&slot_id, &full_prompt, timeout_ms).await"
             "The per-slot dispatch guard MUST be held across the entire state.pty.send call"])))
