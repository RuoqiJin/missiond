(task-lifecycle-event-log wave46-lifecycle-events
  :schema "missiond.task-lifecycle-event.v1"
  :wave wave46
  :created-at "2026-04-29T04:47:14Z"
  :sequence 2

  (lifecycle-event
    :id wave46-lifecycle-bootstrap-start-002
    :task wave46-bootstrap
    :actor_role prepare-task-runner-wave
    :event_kind trace_start
    :commit_role none
    :seq 1
    :at "2026-04-29T04:50:24Z"
    :touched [".missiond/claudecode/wave46-shared-preamble.md"]
    :summary "Bootstrap lifecycle start."
    :legacy_memory_id wave46-bootstrap-002
    :legacy_trace_id wave46-trace-bootstrap-start-002)

  (lifecycle-event
    :id wave46-lifecycle-bootstrap-read-002
    :task wave46-bootstrap
    :actor_role prepare-task-runner-wave
    :event_kind read
    :commit_role none
    :seq 2
    :at "2026-04-29T04:50:24Z"
    :touched [".missiond/claudecode/wave46-shared-preamble.md"]
    :summary "Audit expectation: every worker brief MUST load the shared preamble before broad scans; this entry seeds the preamble-read trace pin so verifiers can detect missing follow-up reads."
    :legacy_trace_id wave46-trace-bootstrap-read-002))
