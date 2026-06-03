(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260603001000-Log-system-webhook-EventBridge-commit"
  :intent "wo-20260603001000-Log-system-webhook-EventBridge-commit"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260603001000-Log-system-webhook-EventBridge-commit-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/main.rs"
                     ".missiond/work-orders/wo-20260603001000-Log-system-webhook-EventBridge-commit"]
       :acceptance ["cargo check -p missiond-daemon"])))
