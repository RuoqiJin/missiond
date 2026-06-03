(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260603222739-Bind-Codex-provider-box-monitori"
  :intent "wo-20260603222739-Bind-Codex-provider-box-monitori"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260603222739-Bind-Codex-provider-box-monitori-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/provider_box/codex_driver.rs"]
       :acceptance ["cargo fmt --all --check"
                    "cargo test -p missiond-daemon rollout_extractor_handles_current_codex_agent_message_shape -- --nocapture"
                    "cargo check -p missiond-daemon"])))
