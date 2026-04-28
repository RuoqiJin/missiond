;; Wave 32 session trace.

(session-trace wave32
  :schema "missiond.session-trace.v1"
  :wave wave32
  :created-at "2026-04-28T20:30:00+08:00"
  :sequence 3

  (trace-event
    :id wave32-trace-bootstrap-001
    :seq 1
    :at "2026-04-28T20:30:00+08:00"
    :task wave32-01-autopilot-timeout-budget-v0
    :backend codex-orchestrator
    :kind dispatch
    :summary "Wave32 generated as a single-worker ClaudeCode stability probe for Autopilot timeout/watchdog alignment.")

  (trace-event
    :id wave32-trace-bootstrap-start-002
    :seq 2
    :at "2026-04-28T12:32:49Z"
    :task wave32-bootstrap
    :backend prepare-task-runner-wave
    :kind start
    :summary "Bootstrap: validated manifest, rendered thin briefs + preamble, scaffolded report skeletons.")

  (trace-event
    :id wave32-trace-bootstrap-read-002
    :seq 3
    :at "2026-04-28T12:32:49Z"
    :task wave32-bootstrap
    :backend prepare-task-runner-wave
    :kind read
    :files [".missiond/claudecode/wave32-shared-preamble.md"]
    :summary "Audit expectation: every worker brief MUST load the shared preamble before broad scans; this entry seeds the preamble-read trace pin so verifiers can detect missing follow-up reads."))
