(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260603111111-Fix-Jarvis-grounding-SSE-heartbe"
  :intent "wo-20260603111111-Fix-Jarvis-grounding-SSE-heartbe"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260603111111-Fix-Jarvis-grounding-SSE-heartbe-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-core/src/ws/server.rs"]
       :acceptance ["cargo check -p missiond-core"])))
