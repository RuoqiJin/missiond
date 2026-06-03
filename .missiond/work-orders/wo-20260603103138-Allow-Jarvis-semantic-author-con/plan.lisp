(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260603103138-Allow-Jarvis-semantic-author-con"
  :intent "wo-20260603103138-Allow-Jarvis-semantic-author-con"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260603103138-Allow-Jarvis-semantic-author-con-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-core/src/ws/server.rs"]
       :acceptance ["cargo test -p missiond-core jarvis_author_response_accepts_numeric_confidence -- --nocapture"])))
