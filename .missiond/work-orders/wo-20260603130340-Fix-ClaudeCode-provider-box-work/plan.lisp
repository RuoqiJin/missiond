(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260603130340-Fix-ClaudeCode-provider-box-work"
  :intent "wo-20260603130340-Fix-ClaudeCode-provider-box-work"
  :status accepted
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260603130340-Fix-ClaudeCode-provider-box-work-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/provider_box/claude_code_driver.rs"]
       :acceptance ["cargo test -p missiond-daemon provider_box::claude_code_driver -- --nocapture"
                    "cargo check -p missiond-daemon"])))
