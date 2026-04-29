(task-lifecycle-event-log wave47-lifecycle-events
  :schema "missiond.task-lifecycle-event.v1"
  :wave wave47
  :created-at "2026-04-29T05:11:25Z"
  :sequence 3

  (lifecycle-event
    :id wave47-lifecycle-bootstrap-start-002
    :task wave47-bootstrap
    :actor_role prepare-task-runner-wave
    :event_kind trace_start
    :commit_role none
    :seq 1
    :at "2026-04-29T05:13:44Z"
    :touched [".missiond/claudecode/wave47-shared-preamble.md"]
    :summary "Bootstrap lifecycle start."
    :legacy_memory_id wave47-bootstrap-002
    :legacy_trace_id wave47-trace-bootstrap-start-002)

  (lifecycle-event
    :id wave47-lifecycle-bootstrap-read-002
    :task wave47-bootstrap
    :actor_role prepare-task-runner-wave
    :event_kind read
    :commit_role none
    :seq 2
    :at "2026-04-29T05:13:44Z"
    :touched [".missiond/claudecode/wave47-shared-preamble.md"]
    :summary "Audit expectation: every worker brief MUST load the shared preamble before broad scans; this entry seeds the preamble-read trace pin so verifiers can detect missing follow-up reads."
    :legacy_trace_id wave47-trace-bootstrap-read-002)

  (lifecycle-event
    :id wave47-01-request-real-dispatch-smoke-v0-dispatch-003
    :task wave47-01-request-real-dispatch-smoke-v0
    :actor_role orchestrator
    :event_kind dispatch
    :commit_role none
    :seq 3
    :at "2026-04-29T05:14:28Z"
    :touched [".missiond/claudecode/wave47-01-request-real-dispatch-smoke-v0.md" ".missiond/tasks/wave47/manifest.lisp"]
    :summary "Dispatch wave47-01-request-real-dispatch-smoke-v0: hard dependencies satisfied."))
