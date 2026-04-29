(task-lifecycle-event-log wave45-lifecycle-events
  :schema "missiond.task-lifecycle-event.v1"
  :wave wave45
  :created-at "2026-04-29T04:30:35Z"
  :sequence 2

  (lifecycle-event
    :id wave45-lifecycle-bootstrap-start-003
    :task wave45-bootstrap
    :actor_role prepare-task-runner-wave
    :event_kind trace_start
    :commit_role none
    :seq 1
    :at "2026-04-29T04:33:20Z"
    :touched [".missiond/claudecode/wave45-shared-preamble.md"]
    :summary "Bootstrap lifecycle start."
    :legacy_memory_id wave45-bootstrap-003
    :legacy_trace_id wave45-trace-bootstrap-start-003)

  (lifecycle-event
    :id wave45-lifecycle-bootstrap-read-003
    :task wave45-bootstrap
    :actor_role prepare-task-runner-wave
    :event_kind read
    :commit_role none
    :seq 2
    :at "2026-04-29T04:33:20Z"
    :touched [".missiond/claudecode/wave45-shared-preamble.md"]
    :summary "Audit expectation: every worker brief MUST load the shared preamble before broad scans; this entry seeds the preamble-read trace pin so verifiers can detect missing follow-up reads."
    :legacy_trace_id wave45-trace-bootstrap-read-003))
