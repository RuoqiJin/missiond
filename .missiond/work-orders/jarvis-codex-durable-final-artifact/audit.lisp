(work-order-audit
  :schema "missiond.work-order.audit.v1"
  :id "jarvis-codex-durable-final-artifact"
  :events ((event created :at "2026-05-24T07:33:28.169Z" :actor missiond-work-order)
           (event accepted :at "2026-05-24T07:48:00Z" :actor codex
             :summary "Fix the observed Codex worker gap: durable final exists, but no task-result-artifact is written and Jarvis follow cannot return a canonical result.")))
