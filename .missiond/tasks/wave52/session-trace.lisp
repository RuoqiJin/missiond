(session-trace wave52
  :schema "missiond.session-trace.v1"
  :wave wave52
  :created-at "2026-04-29T09:12:00Z"
  :sequence 0

  (trace-event
    :id wave52-trace-bootstrap-start-001
    :seq 1
    :at "2026-04-29T09:13:38Z"
    :task wave52-bootstrap
    :backend prepare-task-runner-wave
    :kind start
    :summary "Bootstrap: validated manifest, rendered thin briefs + preamble, scaffolded report skeletons.")

  (trace-event
    :id wave52-trace-bootstrap-read-001
    :seq 2
    :at "2026-04-29T09:13:38Z"
    :task wave52-bootstrap
    :backend prepare-task-runner-wave
    :kind read
    :files [".missiond/claudecode/wave52-shared-preamble.md"]
    :summary "Audit expectation: every worker brief MUST load the shared preamble before broad scans; this entry seeds the preamble-read trace pin so verifiers can detect missing follow-up reads."))
