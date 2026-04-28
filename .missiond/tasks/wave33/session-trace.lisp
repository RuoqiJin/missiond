;; Wave 33 session trace.

(session-trace wave33
  :schema "missiond.session-trace.v1"
  :wave wave33
  :created-at "2026-04-28T20:52:00+08:00"
  :sequence 3

  (trace-event
    :id wave33-trace-bootstrap-001
    :seq 1
    :at "2026-04-28T20:52:00+08:00"
    :task wave33-01-autopilot-prompt-contract-v0
    :backend codex-orchestrator
    :kind dispatch
    :summary "Wave33 generated as a single-worker ClaudeCode code-alignment task for Autopilot prompt/tool contract projection.")

  (trace-event
    :id wave33-trace-bootstrap-start-002
    :seq 2
    :at "2026-04-28T12:53:40Z"
    :task wave33-bootstrap
    :backend prepare-task-runner-wave
    :kind start
    :summary "Bootstrap: validated manifest, rendered thin briefs + preamble, scaffolded report skeletons.")

  (trace-event
    :id wave33-trace-bootstrap-read-002
    :seq 3
    :at "2026-04-28T12:53:40Z"
    :task wave33-bootstrap
    :backend prepare-task-runner-wave
    :kind read
    :files [".missiond/claudecode/wave33-shared-preamble.md"]
    :summary "Audit expectation: every worker brief MUST load the shared preamble before broad scans; this entry seeds the preamble-read trace pin so verifiers can detect missing follow-up reads."))
