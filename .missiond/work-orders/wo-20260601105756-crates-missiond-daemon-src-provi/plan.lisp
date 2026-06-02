(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260601105756-crates-missiond-daemon-src-provi"
  :intent "wo-20260601105756-crates-missiond-daemon-src-provi"
  :status accepted
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260601105756-crates-missiond-daemon-src-provi-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/provider_box/agy_driver.rs"
                     "crates/missiond-daemon/src/provider_box/http_adapter.rs"]
       :acceptance ["cargo test -p missiond-daemon agy_driver -- --nocapture"
                    "cargo test -p missiond-daemon provider_box::http_adapter -- --nocapture"
                    "node scripts/check-v3-interactive-provider-box.mjs --json"])))
