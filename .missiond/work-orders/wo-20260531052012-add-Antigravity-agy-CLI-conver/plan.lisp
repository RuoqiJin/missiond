(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260531052012-add-Antigravity-agy-CLI-conver"
  :intent "wo-20260531052012-add-Antigravity-agy-CLI-conver"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260531052012-add-Antigravity-agy-CLI-conver-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-core/src/agy_cli/"
                     "crates/missiond-core/src/lib.rs"
                     "crates/missiond-daemon/src/main.rs"
                     "crates/missiond-daemon/src/infra/ingestion_router.rs"]
       :acceptance ["node scripts/check-v3-final-convergence.mjs --json --static-only"])))
