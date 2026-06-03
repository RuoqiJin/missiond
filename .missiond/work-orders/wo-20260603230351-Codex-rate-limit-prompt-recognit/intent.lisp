(work-order-intent
  :schema "missiond.work-order.intent.v1"
  :work_order_id "wo-20260603230351-Codex-rate-limit-prompt-recognit"
  :title "Recognize Codex keep-current rate-limit prompts before hard usage-limit blocking"
  :source "jarvis-public-smoke"
  :objective "Allow provider-box Codex author slots to dismiss safe keep-current model prompts instead of failing Jarvis intent authoring as usage_limit."
  :scope
    (:read ["crates/missiond-pty/src/pty_recognition.rs"
            "crates/missiond-daemon/src/provider_box/codex_driver.rs"]
     :write ["crates/missiond-pty/src/pty_recognition.rs"])
  :non_goals ["Do not bypass hard quota exhausted errors."
              "Do not synthesize Jarvis intent or direct answers outside provider-box."
              "Do not expose provider-box slot APIs publicly."]
  :acceptance ["Codex rate-limit prompts with switch/keep-current/confirm choices classify as model_switch_prompt."
               "Hard quota/rate-limit errors without keep-current choices still classify as usage_limit."
               "Jarvis prompt smoke can progress past intent authoring after deployment if the slot is on that prompt."])
