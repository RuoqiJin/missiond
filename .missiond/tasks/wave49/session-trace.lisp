(session-trace wave49
  :schema "missiond.session-trace.v1"
  :wave wave49
  :created-at "2026-04-29T06:30:00Z"
  :sequence 1

  (trace-event
    :id wave49-bootstrap-start-001
    :seq 1
    :at "2026-04-29T06:30:00Z"
    :task wave49-bootstrap
    :backend codex
    :kind dispatch
    :files [".missiond/tasks/wave49/manifest.lisp"
            ".missiond/tasks/wave49/wave49-01-request-flow-restart-recovery-smoke-v0.lisp"]
    :summary "Bootstrap wave49 restart-recovery smoke implementation wave.")

  (trace-event
    :id wave49-trace-bootstrap-start-002
    :seq 2
    :at "2026-04-29T06:29:31Z"
    :task wave49-bootstrap
    :backend prepare-task-runner-wave
    :kind start
    :summary "Bootstrap: validated manifest, rendered thin briefs + preamble, scaffolded report skeletons.")

  (trace-event
    :id wave49-trace-bootstrap-read-002
    :seq 3
    :at "2026-04-29T06:29:31Z"
    :task wave49-bootstrap
    :backend prepare-task-runner-wave
    :kind read
    :files [".missiond/claudecode/wave49-shared-preamble.md"]
    :summary "Audit expectation: every worker brief MUST load the shared preamble before broad scans; this entry seeds the preamble-read trace pin so verifiers can detect missing follow-up reads."))
