(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260601103559-Retry-AGY-text-only-turns-when-s"
  :intent "wo-20260601103559-Retry-AGY-text-only-turns-when-s"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260601103559-Retry-AGY-text-only-turns-when-s-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/provider_box/agy_driver.rs"]
       :acceptance ["cargo test -p missiond-daemon agy_driver -- --nocapture"
                    "node scripts/check-v3-interactive-provider-box.mjs --json"])))
