(task-lifecycle-event-log wave44-lifecycle-events
  :schema "missiond.task-lifecycle-event.v1"
  :wave wave44
  :created-at "2026-04-29T04:08:08Z"
  :sequence 2

  (lifecycle-event
    :id wave44-lifecycle-bootstrap-start-002
    :task wave44-bootstrap
    :actor_role prepare-task-runner-wave
    :event_kind trace_start
    :commit_role none
    :seq 1
    :at "2026-04-29T04:10:38Z"
    :touched [".missiond/claudecode/wave44-shared-preamble.md"]
    :summary "Bootstrap lifecycle start."
    :legacy_memory_id wave44-bootstrap-002
    :legacy_trace_id wave44-trace-bootstrap-start-002)

  (lifecycle-event
    :id wave44-lifecycle-bootstrap-read-002
    :task wave44-bootstrap
    :actor_role prepare-task-runner-wave
    :event_kind read
    :commit_role none
    :seq 2
    :at "2026-04-29T04:10:38Z"
    :touched [".missiond/claudecode/wave44-shared-preamble.md"]
    :summary "Audit expectation: every worker brief MUST load the shared preamble before broad scans; this entry seeds the preamble-read trace pin so verifiers can detect missing follow-up reads."
    :legacy_trace_id wave44-trace-bootstrap-read-002))
