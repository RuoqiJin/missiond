(task-lifecycle-event-log wave50-lifecycle-events
  :schema "missiond.task-lifecycle-event.v1"
  :wave wave50
  :created-at "2026-04-29T08:05:00Z"
  :sequence 3

  (lifecycle-event
    :id wave50-lifecycle-bootstrap-start-002
    :task wave50-bootstrap
    :actor_role prepare-task-runner-wave
    :event_kind trace_start
    :commit_role none
    :seq 1
    :at "2026-04-29T07:00:28Z"
    :touched [".missiond/claudecode/wave50-shared-preamble.md"]
    :summary "Bootstrap lifecycle start."
    :legacy_memory_id wave50-bootstrap-002
    :legacy_trace_id wave50-trace-bootstrap-start-002)

  (lifecycle-event
    :id wave50-lifecycle-bootstrap-read-002
    :task wave50-bootstrap
    :actor_role prepare-task-runner-wave
    :event_kind read
    :commit_role none
    :seq 2
    :at "2026-04-29T07:00:28Z"
    :touched [".missiond/claudecode/wave50-shared-preamble.md"]
    :summary "Audit expectation: every worker brief MUST load the shared preamble before broad scans; this entry seeds the preamble-read trace pin so verifiers can detect missing follow-up reads."
    :legacy_trace_id wave50-trace-bootstrap-read-002)

  (lifecycle-event
    :id wave50-01-board-task-timeout-lease-v0-dispatch-003
    :task wave50-01-board-task-timeout-lease-v0
    :actor_role orchestrator
    :event_kind dispatch
    :commit_role none
    :seq 3
    :at "2026-04-29T07:02:37Z"
    :touched [".missiond/claudecode/wave50-01-board-task-timeout-lease-v0.md" ".missiond/tasks/wave50/manifest.lisp"]
    :summary "Dispatch wave50-01-board-task-timeout-lease-v0: hard dependencies satisfied."))
