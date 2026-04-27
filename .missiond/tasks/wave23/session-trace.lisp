;; Wave 23 session-trace ledger.
;; Schema: .missiond/tasks/schema/session-trace-v1.lisp
;; Checker: scripts/check-session-trace.mjs
;;
;; This file is the seed ledger for Wave 23 task execution traces. It is
;; append-only and fact-oriented. Worker reports may explain why; this trace
;; records what happened.

(session-trace wave23
  :schema "missiond.session-trace.v1"
  :wave wave23
  :created-at "2026-04-28T00:00:00+08:00"
  :sequence 1

  (trace-event
    :id wave23-trace-bootstrap-001
    :seq 1
    :at "2026-04-28T00:00:00+08:00"
    :task wave23-01-session-trace-schema-v0
    :backend codex-orchestrator
    :kind observation
    :summary "Session trace ledger seeded. Future task events should append dispatch/start/command/test/commit/complete/failure facts here."))
