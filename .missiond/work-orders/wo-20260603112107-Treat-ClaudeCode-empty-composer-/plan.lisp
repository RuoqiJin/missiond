(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260603112107-Treat-ClaudeCode-empty-composer-"
  :intent "wo-20260603112107-Treat-ClaudeCode-empty-composer-"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260603112107-Treat-ClaudeCode-empty-composer--shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/provider_box/claude_code_driver.rs"]
       :acceptance ["cargo check -p missiond-daemon"
                    "cargo test -p missiond-daemon claude_code_placeholder_composer_is_treated_as_empty_input -- --nocapture"
                    "cargo test -p missiond-daemon claude_code_real_composer_text_is_preserved -- --nocapture"])))
