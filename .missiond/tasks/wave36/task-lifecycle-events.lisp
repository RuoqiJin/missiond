;; Wave 36 task lifecycle events.

(task-lifecycle-event-log wave36-lifecycle-events
  :schema "missiond.task-lifecycle-event.v1"
  :wave wave36
  :created-at "2026-04-28T22:56:00+08:00"
  :sequence 7

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
    :legacy_trace_id wave36-trace-bootstrap-read-001)

  (lifecycle-event
    :id wave36-01-mission-request-review-response-v0-dispatch-003
    :task wave36-01-mission-request-review-response-v0
    :actor_role orchestrator
    :event_kind dispatch
    :commit_role none
    :seq 3
    :at "2026-04-28T14:56:40Z"
    :touched [".missiond/claudecode/wave36-01-mission-request-review-response-v0.md" ".missiond/tasks/wave36/manifest.lisp"]
    :summary "Dispatch wave36-01-mission-request-review-response-v0: hard dependencies satisfied.")

  (lifecycle-event
    :id wave36-01-mission-request-review-response-v0-claim-004
    :task wave36-01-mission-request-review-response-v0
    :actor_role claudecode
    :event_kind claim
    :commit_role none
    :seq 4
    :at "2026-04-28T15:05:00Z"
    :touched [".missiond/tasks/wave36/wave36-01-mission-request-review-response-v0.lisp"]
    :summary "Worker claim: implementing mission_request action=respond adapter, request-local review events, and V3 review-response contract."
    :legacy_memory_id wave36-01-claim-003
    :legacy_trace_id wave36-trace-01-claim-004)

  (lifecycle-event
    :id wave36-01-mission-request-review-response-v0-read-005
    :task wave36-01-mission-request-review-response-v0
    :actor_role claudecode
    :event_kind read
    :commit_role none
    :seq 5
    :at "2026-04-28T15:05:00Z"
    :touched [".missiond/claudecode/wave36-shared-preamble.md"
              ".missiond/tasks/wave36/context-atlas.lisp"
              ".missiond/tasks/wave36/pattern-cards.lisp"
              ".missiond/v3/missiond-blueprint.lisp"
              "crates/missiond-daemon/src/handlers/knowledge/request.rs"
              "crates/missiond-mcp/src/tools/knowledge/request.rs"]
    :summary "Loaded shared preamble, atlas, pattern card, V3 blueprint, request.rs handler, and MCP tool definition before editing."
    :legacy_trace_id wave36-trace-01-read-005)

  (lifecycle-event
    :id wave36-01-mission-request-review-response-v0-commit-006
    :task wave36-01-mission-request-review-response-v0
    :actor_role claudecode
    :event_kind worker_commit
    :commit_role worker
    :seq 6
    :at "2026-04-28T15:15:12Z"
    :touched [".missiond/v3/missiond-blueprint.lisp"
              "crates/missiond-daemon/src/handlers/knowledge/request.rs"
              "crates/missiond-mcp/src/tools/knowledge/request.rs"]
    :summary "Committed at 37421f4ae3af with message `feat(request): accept mission request review responses` after task-scope-guard staged OK and acceptance commands all green."
    :legacy_trace_id wave36-trace-01-commit-006)

  (lifecycle-event
    :id wave36-01-mission-request-review-response-v0-completion-007
    :task wave36-01-mission-request-review-response-v0
    :actor_role claudecode
    :event_kind completion
    :commit_role none
    :seq 7
    :at "2026-04-28T15:17:58Z"
    :touched [".missiond/tasks/wave36/reports/wave36-01-mission-request-review-response-v0.report.lisp"]
    :summary "Wrote done-status report at commit 37421f4ae3af; check-task-report.mjs PASS; verify-task-contract.mjs PASS."
    :legacy_memory_id wave36-01-completion-004
    :legacy_trace_id wave36-trace-01-completion-007))
