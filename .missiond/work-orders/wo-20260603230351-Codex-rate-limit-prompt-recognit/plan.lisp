(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :work_order_id "wo-20260603230351-Codex-rate-limit-prompt-recognit"
  :intent_ref "intent.lisp"
  :read_scope ["crates/missiond-pty/src/pty_recognition.rs"
               "crates/missiond-daemon/src/provider_box/codex_driver.rs"]
  :write_scope ["crates/missiond-pty/src/pty_recognition.rs"]
  :nodes
    ((node inspect-failure
       :kind investigation
       :status completed
       :evidence "Public Jarvis SSE failed before intent_archived because slot-codex-intent-author was blocked with provider:usage_limit.")
     (node patch-recognition
       :kind code-change
       :status completed
       :serial_after [inspect-failure]
       :evidence "Move safe Codex rate-limit model-switch prompt recognition before generic provider_unavailable_match.")
     (node verify-local
       :kind verification
       :status completed
       :serial_after [patch-recognition]
       :commands ["cargo fmt --all --check"
                  "cargo test -p missiond-pty codex_usage_limit_model_switch_prompt_preempts_hard_usage_limit_block -- --nocapture"
                  "cargo test -p missiond-pty codex_hard_usage_limit_without_keep_current_choice_stays_blocked -- --nocapture"
                  "cargo test -p missiond-daemon codex_rate_limit_prompt_selection_prefers_keep_current_never_show -- --nocapture"])
     (node deploy-and-smoke
       :kind deploy-ops
       :status pending
       :serial_after [verify-local]
       :commands ["git commit"
                  "git push origin main"
                  "MissionD Mac mini self-update"
                  "Jarvis public prompt smoke"])))
  :risk "If Codex changes prompt wording, unknown screens still fail closed as blocked.")
