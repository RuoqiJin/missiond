(session-trace wave50
  :schema "missiond.session-trace.v1"
  :wave wave50
  :created-at "2026-04-29T08:05:00Z"
  :sequence 1

  (trace-event
    :id wave50-bootstrap-start-001
    :seq 1
    :at "2026-04-29T08:05:00Z"
    :task wave50-bootstrap
    :backend codex
    :kind dispatch
    :files [".missiond/tasks/wave50/manifest.lisp"
            ".missiond/tasks/wave50/context-pack.lisp"
            ".missiond/tasks/wave50/wave50-01-board-task-timeout-lease-v0.lisp"]
    :summary "Bootstrap wave50 timeout-derived BoardTask lease implementation wave.")

  (trace-event
    :id wave50-trace-bootstrap-start-002
    :seq 2
    :at "2026-04-29T07:00:28Z"
    :task wave50-bootstrap
    :backend prepare-task-runner-wave
    :kind start
    :summary "Bootstrap: validated manifest, rendered thin briefs + preamble, scaffolded report skeletons.")

  (trace-event
    :id wave50-trace-bootstrap-read-002
    :seq 3
    :at "2026-04-29T07:00:28Z"
    :task wave50-bootstrap
    :backend prepare-task-runner-wave
    :kind read
    :files [".missiond/claudecode/wave50-shared-preamble.md"]
    :summary "Audit expectation: every worker brief MUST load the shared preamble before broad scans; this entry seeds the preamble-read trace pin so verifiers can detect missing follow-up reads."))
