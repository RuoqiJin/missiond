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
    :summary "wave prepared by prepare-task-runner-wave.mjs — briefs + report skeletons + preamble-read audit expectation seeded.")

  (claim
    :id wave32-01-claim-003
    :task wave32-01-autopilot-timeout-budget-v0
    :agent claudecode
    :seq 3
    :at "2026-04-28T12:37:08Z"
    :touched [".missiond/claudecode/wave32-shared-preamble.md"
              ".missiond/tasks/wave32/wave32-01-autopilot-timeout-budget-v0.lisp"
              ".missiond/tasks/wave32/context-atlas.lisp"
              ".missiond/tasks/wave32/pattern-cards.lisp"
              ".missiond/v3/missiond-blueprint.lisp"]
    :summary "Claim wave32-01: replacing fixed 10min pty.send timeout in Autopilot dispatcher with a BoardTask.timeout_secs-derived helper, lifting the watchdog idle threshold to task_timeout+grace, preserving fast no-PTY-session recovery, and recording the projection in v3 workstation-config invariants.")

  (completion
    :id wave32-01-completion-004
    :task wave32-01-autopilot-timeout-budget-v0
    :agent claudecode
    :seq 4
    :at "2026-04-28T12:41:38Z"
    :touched ["crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
              ".missiond/v3/missiond-blueprint.lisp"
              ".missiond/tasks/wave32/reports/wave32-01-autopilot-timeout-budget-v0.report.lisp"]
    :summary "Autopilot pty.send + smart-watchdog now project BoardTask.timeout_secs (default 1800s, clamp 60..7200, watchdog grace 120s, missing-session probe 120s); no-PTY-session early recovery preserved; watchdog wording updated; 9 new pure tests + 2 pre-existing pass; cargo check clean; lisp/blueprint/NUL/whitespace gates green; v3 invariants and implementation-map note refreshed."))
