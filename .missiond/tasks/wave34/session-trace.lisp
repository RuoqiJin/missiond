;; Wave 34 session trace.

(session-trace wave34
  :schema "missiond.session-trace.v1"
  :wave wave34
  :created-at "2026-04-28T21:15:00+08:00"
  :sequence 1

  (trace-event
    :id wave34-trace-bootstrap-001
    :seq 1
    :at "2026-04-28T21:15:00+08:00"
    :task wave34-01-autopilot-complete-close-v0
    :backend codex-orchestrator
    :kind dispatch
    :summary "Wave34 generated as a single-worker ClaudeCode code-alignment task for delegated BoardTask prompt/close ownership.")

  (trace-event
    :id wave34-trace-bootstrap-start-002
    :seq 2
    :at "2026-04-28T13:18:58Z"
    :task wave34-bootstrap
    :backend prepare-task-runner-wave
    :kind start
    :summary "Bootstrap: validated manifest, rendered thin briefs + preamble, scaffolded report skeletons.")

  (trace-event
    :id wave34-trace-bootstrap-read-002
    :seq 3
    :at "2026-04-28T13:18:58Z"
    :task wave34-bootstrap
    :backend prepare-task-runner-wave
    :kind read
    :files [".missiond/claudecode/wave34-shared-preamble.md"]
    :summary "Audit expectation: every worker brief MUST load the shared preamble before broad scans; this entry seeds the preamble-read trace pin so verifiers can detect missing follow-up reads.")

  (trace-event
    :id wave34-01-trace-preamble-read-004
    :seq 4
    :at "2026-04-28T13:25:00Z"
    :task wave34-01-autopilot-complete-close-v0
    :backend claudecode
    :kind read
    :files [".missiond/claudecode/wave34-shared-preamble.md"
            ".missiond/claudecode/wave34-01-autopilot-complete-close-v0.md"
            ".missiond/tasks/wave34/wave34-01-autopilot-complete-close-v0.lisp"
            ".missiond/tasks/wave34/context-atlas.lisp"
            ".missiond/tasks/wave34/pattern-cards.lisp"]
    :summary "Worker loaded shared preamble + thin brief + task contract + atlas + pattern cards before broad scans."))
