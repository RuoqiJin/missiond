;; Wave 32 dispatch-time context atlas.
;; Read-only guidance for the worker. Task contract remains the source of truth.

(context-atlas wave32-autopilot-stability
  :schema "missiond.context-atlas.dispatch.v0"
  :wave wave32
  :goal "Fix the observed wave31 premature re-dispatch path by aligning Autopilot wait budgets with task_delegate timeout_secs."
  :read-order [".missiond/claudecode/wave32-shared-preamble.md"
               ".missiond/tasks/wave32/context-atlas.lisp"
               ".missiond/tasks/wave32/pattern-cards.lisp"
               ".missiond/v3/missiond-blueprint.lisp"
               ".missiond/tasks/wave32/wave32-01-autopilot-timeout-budget-v0.lisp"]

  (global-anchors
    (file "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
      :purpose "Autopilot board task dispatch, PTY send timeout, retry handling, and watchdog recovery."
      :grep ["Watchdog: slot idle but task still running"
             "claimed_age"
             "timeout_ms"
             "state.pty.send"
             "recover_stale_running_tasks"
             "mod tests"])
    (file "crates/missiond-daemon/src/handlers/compute/task_delegate.rs"
      :purpose "Delegation already computes task timeout_secs from manifest estimated_minutes and stores it on board_tasks."
      :grep ["DEFAULT_TIMEOUT_SECS"
             "MAX_TIMEOUT_SECS"
             "timeout_secs"
             "timeoutForMinutes"])
    (file "crates/missiond-core/src/types/board.rs"
      :purpose "BoardTask carries timeout_secs for autopilot reaper and should now also drive PTY wait budget."
      :grep ["timeout_secs"
             "Custom timeout"])
    (file "crates/missiond-core/src/db/pg/board.rs"
      :purpose "Stale running task fallback already uses board_tasks.timeout_secs; keep behavior consistent."
      :grep ["recover_stale_running_tasks"
             "COALESCE(timeout_secs"])
    (file ".missiond/v3/missiond-blueprint.lisp"
      :purpose "Lisp-owned workstation policy should state that dispatch timeout/watchdog policy is projected from task metadata."
      :grep ["workstation-config"
             "coder"
             "invariants"
             "implementation-map"])))
