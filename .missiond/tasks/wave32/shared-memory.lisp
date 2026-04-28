;; Wave 32 shared-memory ledger.

(shared-memory wave32
  :schema "missiond.shared-memory.v1"
  :wave wave32
  :created-at "2026-04-28T20:30:00+08:00"
  :sequence 2

  (observation
    :id wave32-bootstrap-001
    :task wave32-01-autopilot-timeout-budget-v0
    :agent codex-orchestrator
    :seq 1
    :at "2026-04-28T20:30:00+08:00"
    :touched [".missiond/tasks/wave32/manifest.lisp"
              ".missiond/tasks/wave32/context-atlas.lisp"
              ".missiond/tasks/wave32/pattern-cards.lisp"
              ".missiond/tasks/wave32/wave32-01-autopilot-timeout-budget-v0.lisp"]
    :summary "Wave32 theme: stabilize MissionD-to-ClaudeCode execution after wave31 showed fixed 10 minute Autopilot PTY send timeout caused duplicate re-dispatch of an already-completed Opus coding task.")

  (observation
    :id wave32-bootstrap-002
    :task wave32-bootstrap
    :agent prepare-task-runner-wave
    :seq 2
    :at "2026-04-28T12:32:49Z"
    :touched [".missiond/claudecode/wave32-shared-preamble.md"]
    :summary "wave prepared by prepare-task-runner-wave.mjs — briefs + report skeletons + preamble-read audit expectation seeded."))
