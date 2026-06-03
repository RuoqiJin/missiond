(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260603195002-save-AGY-router-fix-from-tempora"
  :intent "wo-20260603195002-save-AGY-router-fix-from-tempora"
  :status active
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260603195002-save-AGY-router-fix-from-tempora-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/provider_box/agy_driver.rs"]
       :acceptance ["git diff --check"])))
