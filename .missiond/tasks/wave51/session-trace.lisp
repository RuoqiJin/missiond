(session-trace wave51
  :schema "missiond.session-trace.v1"
  :wave wave51
  :created-at "2026-04-29T09:25:00Z"
  :sequence 0


  (trace-event
    :id wave51-trace-bootstrap-start-001
    :seq 1
    :at "2026-04-29T08:35:16Z"
    :task wave51-bootstrap
    :backend prepare-task-runner-wave
    :kind start
    :summary "Bootstrap: validated manifest, rendered thin briefs + preamble, scaffolded report skeletons.")

  (trace-event
    :id wave51-trace-bootstrap-read-001
    :seq 2
    :at "2026-04-29T08:35:16Z"
    :task wave51-bootstrap
    :backend prepare-task-runner-wave
    :kind read
    :files [".missiond/claudecode/wave51-shared-preamble.md"]
    :summary "Audit expectation: every worker brief MUST load the shared preamble before broad scans; this entry seeds the preamble-read trace pin so verifiers can detect missing follow-up reads."))
