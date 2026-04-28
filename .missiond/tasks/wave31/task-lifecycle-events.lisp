(task-lifecycle-event-log wave31-lifecycle-events
  :schema "missiond.task-lifecycle-event.v1"
  :wave wave31
  :created-at "2026-04-28T11:22:52Z"
  :sequence 5


  (lifecycle-event
    :id wave31-lifecycle-bootstrap-start-002
    :task wave31-bootstrap
    :actor_role prepare-task-runner-wave
    :event_kind trace_start
    :commit_role none
    :seq 1
    :at "2026-04-28T11:22:52Z"
    :touched [".missiond/claudecode/wave31-shared-preamble.md"]
    :summary "Bootstrap lifecycle start."
    :legacy_memory_id wave31-bootstrap-002
    :legacy_trace_id wave31-trace-bootstrap-start-002)

  (lifecycle-event
    :id wave31-lifecycle-bootstrap-read-002
    :task wave31-bootstrap
    :actor_role prepare-task-runner-wave
    :event_kind read
    :commit_role none
    :seq 2
    :at "2026-04-28T11:22:52Z"
    :touched [".missiond/claudecode/wave31-shared-preamble.md"]
    :summary "Audit expectation: every worker brief MUST load the shared preamble before broad scans; this entry seeds the preamble-read trace pin so verifiers can detect missing follow-up reads."
    :legacy_trace_id wave31-trace-bootstrap-read-002)

  (lifecycle-event
    :id wave31-01-mission-request-local-projections-v0-dispatch-003
    :task wave31-01-mission-request-local-projections-v0
    :actor_role orchestrator
    :event_kind dispatch
    :commit_role none
    :seq 3
    :at "2026-04-28T11:23:47Z"
    :touched [".missiond/claudecode/wave31-01-mission-request-local-projections-v0.md" ".missiond/tasks/wave31/manifest.lisp"]
    :summary "Dispatch wave31-01-mission-request-local-projections-v0: hard dependencies satisfied.")

  (lifecycle-event
    :id wave31-01-mission-request-local-projections-v0-cancelled-004
    :task wave31-01-mission-request-local-projections-v0
    :actor_role codex-orchestrator
    :event_kind cancelled
    :commit_role none
    :seq 4
    :at "2026-04-28T11:55:53Z"
    :touched [".missiond/tasks/wave31/task-lifecycle-events.lisp"]
    :summary "cancelled wrong-model ClaudeCode dispatch; Sonnet slot was terminated before worker commit")

  (lifecycle-event
    :id wave31-01-mission-request-local-projections-v0-dispatch-005
    :task wave31-01-mission-request-local-projections-v0
    :actor_role codex-orchestrator
    :event_kind dispatch
    :commit_role none
    :seq 5
    :at "2026-04-28T11:56:50Z"
    :touched [".missiond/claudecode/wave31-01-mission-request-local-projections-v0.md" ".missiond/tasks/wave31/manifest.lisp"]
    :summary "Dispatch wave31-01-mission-request-local-projections-v0: hard dependencies satisfied."))
