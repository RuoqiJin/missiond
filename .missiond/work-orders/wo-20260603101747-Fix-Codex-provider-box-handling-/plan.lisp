(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260603101747-Fix-Codex-provider-box-handling-"
  :intent "wo-20260603101747-Fix-Codex-provider-box-handling-"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260603101747-Fix-Codex-provider-box-handling--shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-pty/src/pty_recognition.rs"
                     "crates/missiond-daemon/src/provider_box/codex_driver.rs"]
       :acceptance ["cargo test -p missiond-pty codex_rate_limit_model_switch_prompt_is_blocked -- --nocapture"
                    "cargo test -p missiond-daemon codex_rate_limit_prompt_selection_prefers_keep_current_never_show -- --nocapture"])))
