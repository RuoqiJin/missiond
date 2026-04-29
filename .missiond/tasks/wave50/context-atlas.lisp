;; Wave 50 dispatch-time context atlas.

(context-atlas wave50-timeout-derived-lease-v0
  :schema "missiond.context-atlas.dispatch.v0"
  :wave wave50
  :goal "Make BoardTask claim lease derive from BoardTask.timeout_secs, matching pty.send and watchdog budget."
  :read-order [".missiond/claudecode/wave50-shared-preamble.md"
               ".missiond/tasks/wave50/context-pack.lisp"
               ".missiond/tasks/wave50/wave50-01-board-task-timeout-lease-v0.lisp"
               ".missiond/tasks/wave50/context-atlas.lisp"
               ".missiond/tasks/wave50/pattern-cards.lisp"
               "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
               ".missiond/v3/missiond-blueprint.lisp"
               "scripts/check-v3-workstation-config-isomorphism.mjs"]

  (global-anchors
    (file ".missiond/tasks/wave50/context-pack.lisp"
      :purpose "Shard authority for this code worker."
      :grep ["wave50-integration-plan-001"
             "timeout-derived-lease"
             "(group :id A :shards [timeout-derived-lease])"])
    (file "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
      :purpose "Implementation target. The fixed lease is near dispatch_board_tasks after claim_board_task succeeds; helper tests are near existing PTY timeout/watchdog helper tests."
      :grep ["TimeDelta::minutes(20)"
             "derive_pty_timeout_secs"
             "idle_watchdog_threshold_secs"
             "state.pty.send(&slot_id, &full_prompt, timeout_ms).await"])
    (file ".missiond/v3/missiond-blueprint.lisp"
      :purpose "V3 workstation-config invariant text."
      :grep ["Autopilot pty.send budget"
             "watchdog grace"
             "Restart recovery clears stale"])
    (file "scripts/check-v3-workstation-config-isomorphism.mjs"
      :purpose "Source checker that pins blueprint invariant strings into Rust source."
      :grep ["fn derive_pty_timeout_secs"
             "fn idle_watchdog_threshold_secs"
             "TimeDelta::minutes(20)"])))
