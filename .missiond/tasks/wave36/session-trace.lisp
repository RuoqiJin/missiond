;; Wave 36 session trace.

(session-trace wave36-session-trace
  :schema "missiond.session-trace.v1"
  :wave wave36
  :created-at "2026-04-28T22:56:00+08:00"
  :sequence 0

  (trace-event
    :id wave36-trace-bootstrap-start-001
    :seq 1
    :at "2026-04-28T14:55:47Z"
    :task wave36-bootstrap
    :backend prepare-task-runner-wave
    :kind start
    :summary "Bootstrap: validated manifest, rendered thin briefs + preamble, scaffolded report skeletons.")

  (trace-event
    :id wave36-trace-bootstrap-read-001
    :seq 2
    :at "2026-04-28T14:55:47Z"
    :task wave36-bootstrap
    :backend prepare-task-runner-wave
    :kind read
    :files [".missiond/claudecode/wave36-shared-preamble.md"]
    :summary "Audit expectation: every worker brief MUST load the shared preamble before broad scans; this entry seeds the preamble-read trace pin so verifiers can detect missing follow-up reads."))
