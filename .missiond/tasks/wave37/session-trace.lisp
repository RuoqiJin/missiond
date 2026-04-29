;; Wave 37 session trace.

(session-trace wave37
  :schema "missiond.session-trace.v1"
  :wave wave37
  :created-at "2026-04-29T00:00:00+08:00"
  :sequence 1

  (trace-event
    :id wave37-trace-bootstrap-001
    :seq 1
    :at "2026-04-29T00:00:00+08:00"
    :task wave37-01-request-verification-receipt-v0
    :backend codex-orchestrator
    :kind dispatch
    :summary "Wave37 generated as a ClaudeCode code-alignment task for request-local verification receipt projection.")

  (trace-event
    :id wave37-trace-bootstrap-start-002
    :seq 2
    :at "2026-04-29T01:41:23Z"
    :task wave37-bootstrap
    :backend prepare-task-runner-wave
    :kind start
    :summary "Bootstrap: validated manifest, rendered thin briefs + preamble, scaffolded report skeletons.")

  (trace-event
    :id wave37-trace-bootstrap-read-002
    :seq 3
    :at "2026-04-29T01:41:23Z"
    :task wave37-bootstrap
    :backend prepare-task-runner-wave
    :kind read
    :files [".missiond/claudecode/wave37-shared-preamble.md"]
    :summary "Audit expectation: every worker brief MUST load the shared preamble before broad scans; this entry seeds the preamble-read trace pin so verifiers can detect missing follow-up reads."))
