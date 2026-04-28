;; Wave 31 session trace.

(session-trace wave31
  :schema "missiond.session-trace.v1"
  :wave wave31
  :created-at "2026-04-28T19:22:31+08:00"
  :sequence 1

  (trace-event
    :id wave31-trace-bootstrap-001
    :seq 1
    :at "2026-04-28T19:22:31+08:00"
    :task wave31-01-mission-request-local-projections-v0
    :backend codex-orchestrator
    :kind dispatch
    :summary "Wave31 generated as a single-worker ClaudeCode efficiency probe for mission_request request-local projections.")

  (trace-event
    :id wave31-trace-bootstrap-start-002
    :seq 2
    :at "2026-04-28T11:22:52Z"
    :task wave31-bootstrap
    :backend prepare-task-runner-wave
    :kind start
    :summary "Bootstrap: validated manifest, rendered thin briefs + preamble, scaffolded report skeletons.")

  (trace-event
    :id wave31-trace-bootstrap-read-002
    :seq 3
    :at "2026-04-28T11:22:52Z"
    :task wave31-bootstrap
    :backend prepare-task-runner-wave
    :kind read
    :files [".missiond/claudecode/wave31-shared-preamble.md"]
    :summary "Audit expectation: every worker brief MUST load the shared preamble before broad scans; this entry seeds the preamble-read trace pin so verifiers can detect missing follow-up reads."))
