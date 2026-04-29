(task-lifecycle-event-log wave52-lifecycle-events
  :schema "missiond.task-lifecycle-event.v1"
  :wave wave52
  :created-at "2026-04-29T09:12:00Z"
  :sequence 2

  (lifecycle-event
    :id wave52-lifecycle-bootstrap-start-001
    :task wave52-bootstrap
    :actor_role prepare-task-runner-wave
    :event_kind trace_start
    :commit_role none
    :seq 1
    :at "2026-04-29T09:13:38Z"
    :touched [".missiond/claudecode/wave52-shared-preamble.md"]
    :summary "Bootstrap lifecycle start."
    :legacy_memory_id wave52-bootstrap-001
    :legacy_trace_id wave52-trace-bootstrap-start-001)

  (lifecycle-event
    :id wave52-lifecycle-bootstrap-read-001
    :task wave52-bootstrap
    :actor_role prepare-task-runner-wave
    :event_kind read
    :commit_role none
    :seq 2
    :at "2026-04-29T09:13:38Z"
    :touched [".missiond/claudecode/wave52-shared-preamble.md"]
    :summary "Audit expectation: every worker brief MUST load the shared preamble before broad scans; this entry seeds the preamble-read trace pin so verifiers can detect missing follow-up reads."
    :legacy_trace_id wave52-trace-bootstrap-read-001))
