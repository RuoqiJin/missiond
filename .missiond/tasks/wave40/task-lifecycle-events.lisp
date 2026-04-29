(task-lifecycle-event-log wave40-lifecycle-events
  :schema "missiond.task-lifecycle-event.v1"
  :wave wave40
  :created-at "2026-04-29T02:55:06Z"
  :sequence 3

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
    :legacy_trace_id wave40-trace-bootstrap-read-002)

  (lifecycle-event
    :id wave40-01-parent-hotfix-report-preservation-v0-dispatch-003
    :task wave40-01-parent-hotfix-report-preservation-v0
    :actor_role orchestrator
    :event_kind dispatch
    :commit_role none
    :seq 3
    :at "2026-04-29T02:58:25Z"
    :touched [".missiond/claudecode/wave40-01-parent-hotfix-report-preservation-v0.md" ".missiond/tasks/wave40/manifest.lisp"]
    :summary "Dispatch wave40-01-parent-hotfix-report-preservation-v0: hard dependencies satisfied."))
