(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260531102825-Fix-Codex-provider-box-capabilit"
  :intent "wo-20260531102825-Fix-Codex-provider-box-capabilit"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260531102825-Fix-Codex-provider-box-capabilit-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/provider_box/codex_driver.rs"]
       :acceptance ["cargo check -p missiond-daemon"
                    "git diff --check"])))
