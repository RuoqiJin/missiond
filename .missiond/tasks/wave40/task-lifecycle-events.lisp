(task-lifecycle-event-log wave40-lifecycle-events
  :schema "missiond.task-lifecycle-event.v1"
  :wave wave40
  :created-at "2026-04-29T02:55:06Z"
  :sequence 2

  (lifecycle-event
    :id wave40-lifecycle-bootstrap-start-002
    :task wave40-bootstrap
    :actor_role prepare-task-runner-wave
    :event_kind trace_start
    :commit_role none
    :seq 1
    :at "2026-04-29T02:57:18Z"
    :touched [".missiond/claudecode/wave40-shared-preamble.md"]
    :summary "Bootstrap lifecycle start."
    :legacy_memory_id wave40-bootstrap-002
    :legacy_trace_id wave40-trace-bootstrap-start-002)

  (lifecycle-event
    :id wave40-lifecycle-bootstrap-read-002
    :task wave40-bootstrap
    :actor_role prepare-task-runner-wave
    :event_kind read
    :commit_role none
    :seq 2
    :at "2026-04-29T02:57:18Z"
    :touched [".missiond/claudecode/wave40-shared-preamble.md"]
    :summary "Audit expectation: every worker brief MUST load the shared preamble before broad scans; this entry seeds the preamble-read trace pin so verifiers can detect missing follow-up reads."
    :legacy_trace_id wave40-trace-bootstrap-read-002))
