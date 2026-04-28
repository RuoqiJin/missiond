;; Wave 32 dispatch plan.

(dispatch-plan wave32
  :schema "missiond.dispatch-plan.v0"
  :policy "productive-only"
  :shared-preamble ".missiond/claudecode/wave32-shared-preamble.md"
  :brief-mode thin
  :mainline "Stabilize MissionD -> ClaudeCode task execution after wave31 exposed a fixed 10 minute PTY send timeout."
  :measurement-goal "Observe whether the default Opus 4.7 dynamic coder slot can complete without premature watchdog re-dispatch once timeout policy is code-aligned."

  :nodes
    [(node wave32-01-autopilot-timeout-budget-v0
       :group A
       :verification-tier local
       :estimated-minutes 35
       :write-scope ["crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
                     ".missiond/v3/missiond-blueprint.lisp"])])
