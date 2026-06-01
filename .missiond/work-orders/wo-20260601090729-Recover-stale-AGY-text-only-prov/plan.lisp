(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260601090729-Recover-stale-AGY-text-only-prov"
  :intent "wo-20260601090729-Recover-stale-AGY-text-only-prov"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260601090729-Recover-stale-AGY-text-only-prov-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/provider_box/agy_driver.rs"
                     ".missiond/work-orders/wo-20260601090729-Recover-stale-AGY-text-only-prov/**"]
       :acceptance ["cargo test -p missiond-daemon agy_driver -- --nocapture"
                    "node scripts/check-v3-interactive-provider-box.mjs --json"
                    "node scripts/check-v3-macmini-self-update-lane.mjs --json"])))
