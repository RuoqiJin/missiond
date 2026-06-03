(work-order-audit
  :schema "missiond.work-order.audit.v1"
  :work_order_id "wo-20260603230351-Codex-rate-limit-prompt-recognit"
  :created_at "2026-06-03T23:03:51Z"
  :events
    ((event failure-observed
       :kind runtime-smoke
       :summary "Jarvis real prompt returned typed diagnostic before intent_archived; provider-box reported slot-codex-intent-author blocked with provider:usage_limit.")
     (event patch
       :kind code-change
       :summary "Codex recognition now treats safe switch/keep-current/confirm limit prompts as codex:rate_limit_model_switch_prompt before generic usage_limit matching.")
     (event local-verification
       :kind test
       :summary "Focused pty recognition and provider-box selection tests passed.")))
