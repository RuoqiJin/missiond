;; Wave 32 dispatch-time pattern cards.

(pattern-cards wave32-autopilot-stability
  :schema "missiond.pattern-cards.dispatch.v0"
  :wave wave32

  (card timeout-budget-alignment
    :use-for [wave32-01-autopilot-timeout-budget-v0]
    :problem "wave31 used task_delegate timeout_secs ~=55min but Autopilot pty.send still used a fixed 10min timeout. ClaudeCode completed after 16m54s, then watchdog saw the slot idle and re-dispatched the already-complete board task."
    :recipe ["Do not hardcode 600_000ms for coding board tasks."
             "Derive pty.send timeout from BoardTask.timeout_secs, using the same default/clamp shape as task_delegate when the field is absent or malformed."
             "Make the smart watchdog wait at least the task timeout plus a small grace before unclaiming an idle slot as orphaned."
             "Keep no-PTY-session recovery conservative: if the slot process is absent, it can still unclaim."
             "Add pure unit tests for timeout derivation and watchdog threshold logic; AppState is hard to construct, so test helpers directly."]
    :known-good ["crates/missiond-daemon/src/handlers/compute/task_delegate.rs"
                 "crates/missiond-core/src/db/pg/board.rs"
                 "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"]))
