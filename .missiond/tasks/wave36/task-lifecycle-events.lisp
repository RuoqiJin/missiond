;; Wave 36 task lifecycle events.

(task-lifecycle-event-log wave36-lifecycle-events
  :schema "missiond.task-lifecycle-event.v1"
  :wave wave36
  :created-at "2026-04-28T22:56:00+08:00"
  :sequence 2

  (lifecycle-event
    :id wave36-lifecycle-bootstrap-start-001
    :task wave36-bootstrap
    :actor_role prepare-task-runner-wave
    :event_kind trace_start
    :commit_role none
    :seq 1
    :at "2026-04-28T14:55:47Z"
    :touched [".missiond/claudecode/wave36-shared-preamble.md"]
    :summary "Bootstrap lifecycle start."
    :legacy_memory_id wave36-bootstrap-001
    :legacy_trace_id wave36-trace-bootstrap-start-001)

  (lifecycle-event
    :id wave36-lifecycle-bootstrap-read-001
    :task wave36-bootstrap
    :actor_role prepare-task-runner-wave
    :event_kind read
    :commit_role none
    :seq 2
    :at "2026-04-28T14:55:47Z"
    :touched [".missiond/claudecode/wave36-shared-preamble.md"]
    :summary "Audit expectation: every worker brief MUST load the shared preamble before broad scans; this entry seeds the preamble-read trace pin so verifiers can detect missing follow-up reads."
    :legacy_trace_id wave36-trace-bootstrap-read-001))
