;; Wave 34 dispatch plan.

(dispatch-plan wave34-autopilot-completion-ownership
  :schema "missiond.dispatch-plan.v1"
  :wave wave34
  :goal "Close the next Lisp-isomorphism runtime gap: delegated BoardTask execution must have one prompt owner and one completion owner."
  :ready-queue true
  :max-parallel 1
  :measurement-goal "After wave33, confirm the delegated Opus slot receives exactly the Autopilot task prompt, returns Complete to the owner waiting on pty.send, and leaves the BoardTask done without manual SQL repair."
  :notes
    ["This wave is deliberately narrow: no new product feature, only runtime ownership projection."
     "The observed bug: compute_slot/spawner sent a dynamic slot objective with send_fire_and_forget before Autopilot sent the real BoardTask prompt via pty.send."
     "The second observed bug: PTY emitted Complete, but Autopilot did not transition the BoardTask from running to done; the next task must make that closure path auditable."])
