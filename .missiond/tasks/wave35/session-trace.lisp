;; Wave 35 session trace.

(session-trace wave35
  :schema "missiond.session-trace.v1"
  :wave wave35
  :created-at "2026-04-28T22:34:20+08:00"
  :sequence 1

  (trace-event
    :id wave35-trace-bootstrap-001
    :seq 1
    :at "2026-04-28T22:34:20+08:00"
    :task wave35-01-mission-request-review-packet-v0
    :backend codex-orchestrator
    :kind dispatch
    :summary "Wave35 generated as a single-worker ClaudeCode code-alignment task for request-local review packet projection.")

  (trace-event
    :id wave35-trace-bootstrap-start-002
    :seq 2
    :at "2026-04-28T14:35:36Z"
    :task wave35-bootstrap
    :backend prepare-task-runner-wave
    :kind start
    :summary "Bootstrap: validated manifest, rendered thin briefs + preamble, scaffolded report skeletons.")

  (trace-event
    :id wave35-trace-bootstrap-read-002
    :seq 3
    :at "2026-04-28T14:35:36Z"
    :task wave35-bootstrap
    :backend prepare-task-runner-wave
    :kind read
    :files [".missiond/claudecode/wave35-shared-preamble.md"]
    :summary "Audit expectation: every worker brief MUST load the shared preamble before broad scans; this entry seeds the preamble-read trace pin so verifiers can detect missing follow-up reads."))
