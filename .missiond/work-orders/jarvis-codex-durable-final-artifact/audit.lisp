(work-order-audit
  :schema "missiond.work-order.audit.v1"
  :id "jarvis-codex-durable-final-artifact"
  :events ((event created :at "2026-05-24T07:33:28.169Z" :actor missiond-work-order)
           (event accepted :at "2026-05-24T07:48:00Z" :actor codex
             :summary "Fix the observed Codex worker gap: durable final exists, but no task-result-artifact is written and Jarvis follow cannot return a canonical result.")
           (event scope-expanded :at "2026-05-24T12:37:00Z" :actor codex
             :summary "Runtime evidence showed the real Codex rollout conversation was not task_id-bound: Autopilot only saw the placeholder PTY conversation. Expanded write_scope to include conversation task lookup, Codex task_complete ingestion, and the Autopilot checker.")))
